use crate::{
    app::session::Project,
    core::implementation::{Implementation, TaskRepositoryKind},
    execution::{contract::WorkspaceRef, remote::WorkspaceManifestV1},
    ui::task_detail::RemotePrivateFileGrantCandidate,
};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

struct CachedScan {
    refreshed_at: Option<Instant>,
    loading: bool,
    files: Vec<RemotePrivateFileGrantCandidate>,
    error: Option<String>,
}

static SCANS: OnceLock<Mutex<HashMap<String, CachedScan>>> = OnceLock::new();
const REFRESH_AFTER: Duration = Duration::from_secs(30);
const RETAIN_FOR: Duration = Duration::from_secs(300);
const MAX_CACHE_ENTRIES: usize = 64;
const MAX_CONCURRENT_SCANS: usize = 4;

pub(super) fn for_task(
    project: &Project,
    ticket: &str,
    implementation: Option<&Implementation>,
) -> (Vec<RemotePrivateFileGrantCandidate>, Option<String>, bool) {
    let Some(implementation) = implementation.filter(|state| {
        state.task_repository_kind == TaskRepositoryKind::Clone && state.task_repository_ready
    }) else {
        return (Vec::new(), None, false);
    };
    let task_id = implementation
        .task_uid
        .clone()
        .unwrap_or_else(|| super::super::super::task_target::key(project, ticket));
    let key = format!(
        "{}|{}|{}|{}",
        implementation.workspace_uid.as_deref().unwrap_or_default(),
        task_id,
        implementation.base_commit,
        implementation.task_repository.display(),
    );
    let scans = SCANS.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut scans) = scans.lock() else {
        return (
            Vec::new(),
            Some("Private-file scan cache is unavailable".into()),
            false,
        );
    };
    scans.retain(|_, scan| {
        scan.loading
            || scan
                .refreshed_at
                .is_some_and(|updated| updated.elapsed() < RETAIN_FOR)
    });
    if !scans.contains_key(&key) && scans.len() >= MAX_CACHE_ENTRIES {
        let oldest = scans
            .iter()
            .filter(|(_, scan)| !scan.loading)
            .min_by_key(|(_, scan)| scan.refreshed_at)
            .map(|(key, _)| key.clone());
        let Some(oldest) = oldest else {
            return (
                Vec::new(),
                Some("Private-file scan capacity is busy; retry shortly".into()),
                false,
            );
        };
        scans.remove(&oldest);
    }
    let active_scans = scans.values().filter(|scan| scan.loading).count();
    let scan = scans.entry(key.clone()).or_insert_with(|| CachedScan {
        refreshed_at: None,
        loading: false,
        files: Vec::new(),
        error: None,
    });
    if !scan.loading
        && scan
            .refreshed_at
            .is_none_or(|updated| updated.elapsed() >= REFRESH_AFTER)
    {
        if active_scans >= MAX_CONCURRENT_SCANS {
            scan.files.clear();
            scan.error = Some("Private-file scan capacity is busy; retry shortly".into());
            scan.refreshed_at = Some(Instant::now());
            return (scan.files.clone(), scan.error.clone(), false);
        }
        scan.loading = true;
        scan.files.clear();
        scan.error = None;
        let task = implementation.clone();
        let task_id = task_id.clone();
        let project_id = task.workspace_uid.clone();
        let cache_key = key.clone();
        let spawn = std::thread::Builder::new()
            .name("private-file-scan".into())
            .spawn(move || {
                let result = scan_task(&task, project_id, task_id);
                if let Some(scans) = SCANS.get()
                    && let Ok(mut scans) = scans.lock()
                    && let Some(scan) = scans.get_mut(&cache_key)
                {
                    match result {
                        Ok(files) => {
                            scan.files = files;
                            scan.error = None;
                        }
                        Err(error) => {
                            scan.files.clear();
                            scan.error = Some(error.to_string());
                        }
                    }
                    scan.refreshed_at = Some(Instant::now());
                    scan.loading = false;
                }
            });
        if let Err(error) = spawn {
            scan.loading = false;
            scan.error = Some(format!("Private-file scan could not start: {error}"));
            scan.refreshed_at = Some(Instant::now());
        }
    }
    (scan.files.clone(), scan.error.clone(), scan.loading)
}

fn scan_task(
    implementation: &Implementation,
    project_id: Option<String>,
    task_id: String,
) -> anyhow::Result<Vec<RemotePrivateFileGrantCandidate>> {
    crate::core::implementation::validate_app_owned_task_clone(implementation)?;
    let workspace = WorkspaceRef {
        project_id: project_id
            .ok_or_else(|| anyhow::anyhow!("Task workspace identity is unavailable"))?,
        task_id,
        source_sha: implementation.base_commit.clone(),
    };
    let snapshot = WorkspaceManifestV1::capture(workspace.clone(), &implementation.task_repository)
        .map_err(|error| anyhow::anyhow!("Could not verify the task workspace: {error}"))?;
    Ok(
        crate::execution::remote::private_files::discover_private_files(
            &workspace,
            &implementation.task_repository,
            &snapshot,
        )?
        .into_iter()
        .map(|file| RemotePrivateFileGrantCandidate {
            path: file.path,
            size_bytes: file.size_bytes,
            sha256: file.sha256,
            data_class: file.data_class,
        })
        .collect(),
    )
}
