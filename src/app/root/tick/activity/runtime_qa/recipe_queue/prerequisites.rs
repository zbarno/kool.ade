use crate::{
    execution::{
        contract::{ExecutionModeV1, ResourceBudgetV1, WorkspaceRef},
        docker::images::RuntimeProfile,
        errors::ExecutionErrorKind,
        session::ExecutionSession,
    },
    harness::{DependencyKind, DependencyNeed, PackageEcosystem},
};
use std::{
    path::Path,
    sync::{Arc, Mutex, atomic::AtomicBool, mpsc::Sender},
};

mod dependencies;
#[cfg(test)]
mod tests;
use dependencies::system_packages_for_requirements;

pub(super) struct ConnectRequest<'a> {
    pub(super) workspace: WorkspaceRef,
    pub(super) clone_path: &'a Path,
    pub(super) project_relative_root: &'a str,
    pub(super) target_id: &'a str,
    pub(super) required_tools: &'a [String],
    pub(super) selected_dependency_cache_refs:
        &'a [(String, crate::execution::contract::DependencyCacheRefV1)],
    pub(super) limits: ResourceBudgetV1,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) progress: Sender<crate::harness::LiveProgress>,
    pub(super) session_slot: Arc<Mutex<Option<Arc<ExecutionSession>>>>,
}

pub(super) fn connect(
    request: ConnectRequest<'_>,
) -> Result<
    (
        Arc<ExecutionSession>,
        Option<crate::harness::resource_bridge::ResourceBridge>,
    ),
    String,
> {
    let ConnectRequest {
        workspace,
        clone_path,
        project_relative_root,
        target_id,
        required_tools,
        selected_dependency_cache_refs,
        limits,
        cancel,
        progress,
        session_slot,
    } = request;
    let mut missing_tools = Vec::new();
    let session = match ExecutionSession::connect_local_with_tools(
        workspace.clone(),
        clone_path,
        ExecutionModeV1::Verification,
        required_tools,
        limits.clone(),
    ) {
        Ok(session) => Arc::new(session),
        Err(error) if error.kind == ExecutionErrorKind::ImageUnavailable => {
            let session = Arc::new(
                ExecutionSession::connect_local(
                    workspace.clone(),
                    clone_path,
                    ExecutionModeV1::Verification,
                    RuntimeProfile::General,
                    limits,
                )
                .map_err(|error| {
                    format!(
                        "isolated base recipe executor unavailable: {}",
                        error.message
                    )
                })?,
            );
            missing_tools = session
                .missing_locked_tools(required_tools)
                .map_err(|error| {
                    format!(
                        "could not inspect the selected recipe image: {}",
                        error.message
                    )
                })?;
            if missing_tools.is_empty() && selected_dependency_cache_refs.is_empty() {
                return Err("no locked recipe image provides the requested tools, but the selected base image reports no missing tools".into());
            }
            session
        }
        Err(error) => {
            return Err(format!(
                "isolated recipe executor unavailable: {}",
                error.message
            ));
        }
    };
    publish_session(&session_slot, &session)?;
    let session_arch = session
        .cache_target_arch()
        .ok_or_else(|| "selected recipe image architecture is unavailable".to_owned())?;
    let requested_dependency_ids = selected_dependency_cache_refs
        .iter()
        .map(|(request_id, _)| request_id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut matching_caches = std::collections::BTreeMap::<&str, Vec<_>>::new();
    for (request_id, cache_ref) in selected_dependency_cache_refs {
        let cache_arch =
            crate::execution::docker::discovery::parse_architecture(&cache_ref.target_arch);
        if cache_arch == Some(session_arch.as_str()) {
            matching_caches
                .entry(request_id.as_str())
                .or_default()
                .push(cache_ref);
        }
    }
    let mut prepared_dependency_ids = Vec::with_capacity(requested_dependency_ids.len() + 1);
    for request_id in requested_dependency_ids {
        let candidates = matching_caches.remove(request_id).unwrap_or_default();
        if candidates.len() != 1 {
            return Err(
                "selected target dependency has no unique cache for the recipe image architecture"
                    .into(),
            );
        }
        let cache_ref = candidates[0];
        let cache_store = crate::execution::DependencyCacheSnapshotStore::new(
            crate::persistence::state_root().join("runtime-qa-dependency-caches"),
        );
        let snapshot = cache_store
            .get(request_id, cache_ref)
            .map_err(|error| {
                format!(
                    "selected target dependency cache is unavailable or failed integrity validation: {error}"
                )
            })?;
        session
            .stage_dependency_cache_for_request(request_id, snapshot.clone())
            .map_err(|error| {
                format!(
                    "selected target dependency cache could not be staged: {}",
                    error.message
                )
            })?;
        prepared_dependency_ids.push(request_id.to_owned());
    }
    if missing_tools.is_empty() {
        session
            .record_prepared_dependency_ids(&prepared_dependency_ids)
            .map_err(|error| {
                format!(
                    "prepared target dependency evidence could not be bound to the recipe session: {}",
                    error.message
                )
            })?;
        return Ok((session, None));
    }
    let target_root = safe_target_root(clone_path, project_relative_root)?;
    let bridge = crate::harness::resource_bridge::ResourceBridge::start_for_task_repository_target(
        clone_path,
        &target_root,
        None,
        Some(&workspace.task_id),
        progress,
        cancel,
    )
    .map_err(|error| format!("task dependency broker could not start: {error:#}"))?;
    bridge
        .bind_execution_session(Arc::clone(&session))
        .map_err(|error| {
            format!("task dependency broker could not bind the recipe session: {error:#}")
        })?;
    let system_packages = system_packages_for_requirements(&missing_tools)?;
    if !system_packages.is_empty() {
        let primary = system_packages
            .first()
            .ok_or("runtime recipe has no resolvable system prerequisites")?;
        let need = DependencyNeed {
            ecosystem: PackageEcosystem::System,
            package: Some(primary.clone()),
            version: None,
            source: Some("debian:bookworm".into()),
            command: format!("apt-get install -y {}", system_packages.join(" ")),
            reason: format!(
                "Runtime target {target_id} requires missing tools [{}]",
                missing_tools.join(", ")
            ),
            kind: DependencyKind::SystemTool,
            lockfile_identity: None,
            introduced_packages: Vec::new(),
        };
        let request = bridge.request_system_tools(need)?;
        prepared_dependency_ids.push(request.id);
    }
    session
        .record_prepared_dependency_ids(&prepared_dependency_ids)
        .map_err(|error| {
            format!(
                "prepared target dependency evidence could not be bound to the recipe session: {}",
                error.message
            )
        })?;
    Ok((session, Some(bridge)))
}

fn safe_target_root(clone_path: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
    let clone = clone_path
        .canonicalize()
        .map_err(|error| format!("task clone root is unavailable: {error}"))?;
    let target = clone
        .join(relative)
        .canonicalize()
        .map_err(|error| format!("selected target root is unavailable: {error}"))?;
    if !target.starts_with(&clone) {
        return Err("selected target root resolves outside the task clone".into());
    }
    Ok(target)
}

fn publish_session(
    session_slot: &Mutex<Option<Arc<ExecutionSession>>>,
    session: &Arc<ExecutionSession>,
) -> Result<(), String> {
    *session_slot
        .lock()
        .map_err(|_| "recipe worker cancellation state is unavailable".to_owned())? =
        Some(Arc::clone(session));
    Ok(())
}
