use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn manager_execution_workspace(
    project: &crate::app::session::Project,
    task_id: &str,
) -> Option<crate::execution::contract::WorkspaceRef> {
    crate::core::source_workspace::bind(&project.state, task_id)
}

#[cfg(test)]
pub(crate) fn verify_manager_investigation_receipts_for_test(
    authorization: &crate::core::investigation_broker::InvestigationAuthorizationV1,
) -> Result<Vec<(String, String, String)>, String> {
    super::execute::verified_broker_evidence(authorization)
}

pub(super) fn manager_investigation_authorization(
    project: &crate::app::session::Project,
    workspace: &crate::execution::contract::WorkspaceRef,
    receipt_root: &std::path::Path,
) -> Option<crate::core::investigation_broker::InvestigationAuthorizationV1> {
    use crate::core::repository_knowledge::{ProfileRequest, scan_profile};
    let repository = project
        .state
        .repositories
        .repositories
        .iter()
        .find(|repo| repo.id == "root")?;
    let root = project.state.repo_root.clone();
    let branch = std::process::Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(&root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .ok()?;
    if !branch.status.success() {
        return None;
    }
    let branch = String::from_utf8(branch.stdout).ok()?.trim().to_owned();
    if branch.is_empty() {
        return None;
    }
    let source_ref = format!("refs/heads/{branch}");
    let profile = scan_profile(ProfileRequest {
        repository_id: &repository.id,
        source_ref: &source_ref,
        code_root: &root,
        planning_store: &project.state.planning_store,
        repositories: &project.state.repositories,
        now_unix_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis()
            .min(u64::MAX as u128) as u64,
    })
    .ok()?;
    if profile.key.commit_sha != workspace.source_sha {
        return None;
    }
    Some(
        crate::core::investigation_broker::InvestigationAuthorizationV1 {
            code_root: root,
            planning_root: project.state.planning_store.root.clone(),
            receipt_root: receipt_root.to_path_buf(),
            project_id: workspace.project_id.clone(),
            repository_id: repository.id.clone(),
            source_ref,
            source_sha: workspace.source_sha.clone(),
            workspace_id: format!(
                "{}:{}:{}",
                workspace.project_id, workspace.task_id, workspace.source_sha
            ),
            task_id: workspace.task_id.clone(),
            cancellation_id: workspace.task_id.clone(),
            task_revision: project.state.planning_store.revision().ok()?,
            profile,
            repositories: project.state.repositories.clone(),
            approved_comparison_revisions: approved_comparison_revisions(project, &repository.id),
        },
    )
}

fn approved_comparison_revisions(
    project: &crate::app::session::Project,
    repository_id: &str,
) -> Vec<crate::core::investigation_broker::InvestigationApprovedRevisionV1> {
    approved_comparison_pins(
        project
            .task_documents
            .iter()
            .filter_map(|document| document.metadata.as_ref()),
        repository_id,
    )
}

fn approved_comparison_pins<'a>(
    metadata_items: impl IntoIterator<Item = &'a crate::artifacts::task_docs::TaskMetadata>,
    repository_id: &str,
) -> Vec<crate::core::investigation_broker::InvestigationApprovedRevisionV1> {
    let mut pins = std::collections::BTreeMap::new();
    for metadata in metadata_items {
        let Some(intent) = metadata.git_intent.as_ref() else {
            continue;
        };
        if intent.validate().is_err() || intent.repository_id != repository_id {
            continue;
        }
        let source_ref = if intent.source_ref.starts_with("refs/heads/") {
            intent.source_ref.clone()
        } else {
            format!("refs/heads/{}", intent.source_ref)
        };
        pins.entry((source_ref, intent.source_sha.clone()))
            .or_insert_with(|| {
                crate::core::investigation_broker::InvestigationApprovedRevisionV1 {
                    intent: intent.clone(),
                }
            });
    }
    pins.into_values().take(8).collect()
}

#[cfg(test)]
#[path = "investigation/tests.rs"]
mod tests;

pub(super) fn manager_receipt_root(task_id: &str) -> Option<std::path::PathBuf> {
    let path = std::env::temp_dir().join(format!("koolade-investigation-{task_id}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = std::fs::DirBuilder::new();
        builder.mode(0o700).create(&path).ok()?;
    }
    #[cfg(not(unix))]
    std::fs::create_dir(&path).ok()?;
    Some(path)
}

pub(super) fn watch_manager_cancellation(
    cancelled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    root: std::path::PathBuf,
    cancellation_id: String,
) {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Acquire) {
            if cancelled.load(Ordering::Acquire) {
                write_cancellation_marker(&root, &cancellation_id);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });
}

pub(super) fn write_cancellation_marker(root: &std::path::Path, cancellation_id: &str) {
    let path = root.join(format!(".cancel-{cancellation_id}.flag"));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let _ = options.open(path);
}

pub(super) fn cleanup_after_cancellation_ack(root: std::path::PathBuf, cancellation_id: String) {
    std::thread::spawn(move || {
        let ack = root.join(format!(".cancel-{cancellation_id}.ack"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
        while std::time::Instant::now() < deadline {
            if ack.is_file() {
                let _ = std::fs::remove_dir_all(root);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        // Keep the cancellation marker if the provider was terminated before
        // acknowledging it; a late MCP call must still observe cancellation.
    });
}
