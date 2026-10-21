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
mod qa_caches;
#[cfg(test)]
mod tests;
use dependencies::{parse_required_dependencies, system_packages_for_requirements};
pub(crate) use qa_caches::prepare_qa_dependency_caches;

pub(super) struct ConnectRequest<'a> {
    pub(super) workspace: WorkspaceRef,
    pub(super) clone_path: &'a Path,
    pub(super) project_relative_root: &'a str,
    pub(super) target_id: &'a str,
    pub(super) required_tools: &'a [String],
    pub(super) required_dependencies: &'a [String],
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
        required_dependencies,
        limits,
        cancel,
        progress,
        session_slot,
    } = request;
    let dependencies = parse_required_dependencies(required_dependencies)?;
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
            if missing_tools.is_empty() && dependencies.is_empty() {
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
    if missing_tools.is_empty() && dependencies.is_empty() {
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
    let system_dependencies = dependencies
        .iter()
        .filter(|dependency| dependency.ecosystem == PackageEcosystem::System)
        .collect::<Vec<_>>();
    let system_packages = system_packages_for_requirements(&missing_tools, &system_dependencies)?;
    let mut prepared_dependency_ids = Vec::new();
    if !system_packages.is_empty() {
        let primary = system_packages
            .first()
            .ok_or("runtime recipe has no resolvable system prerequisites")?;
        let dependency_ids = system_dependencies
            .iter()
            .map(|dependency| dependency.id.as_str())
            .collect::<Vec<_>>();
        let need = DependencyNeed {
            ecosystem: PackageEcosystem::System,
            package: Some(primary.clone()),
            version: None,
            source: Some("debian:bookworm".into()),
            command: format!("apt-get install -y {}", system_packages.join(" ")),
            reason: format!(
                "Runtime target {target_id} requires missing tools [{}] and declared system dependencies [{}]",
                missing_tools.join(", "),
                dependency_ids.join(", ")
            ),
            kind: DependencyKind::SystemTool,
            lockfile_identity: None,
            introduced_packages: Vec::new(),
        };
        bridge.request_system_tools(need)?;
        prepared_dependency_ids.extend(
            system_dependencies
                .iter()
                .map(|dependency| dependency.id.clone()),
        );
    }
    for ecosystem in [PackageEcosystem::Npm, PackageEcosystem::Cargo] {
        let packages = dependencies
            .iter()
            .filter(|dependency| dependency.ecosystem == ecosystem)
            .collect::<Vec<_>>();
        if packages.is_empty() {
            continue;
        }
        for dependency in &packages {
            let present = crate::harness::resource_bridge::has_locked_package(
                &target_root,
                ecosystem,
                &dependency.package,
            )
            .map_err(|error| {
                format!(
                    "could not verify locked target dependency {}: {error:#}",
                    dependency.id
                )
            })?;
            if !present {
                return Err(format!(
                    "target dependency {} is absent from the target's verified lockfile",
                    dependency.id
                ));
            }
        }
        let ids = packages
            .iter()
            .map(|dependency| dependency.id.as_str())
            .collect::<Vec<_>>();
        let need = match ecosystem {
            PackageEcosystem::Npm => DependencyNeed {
                ecosystem,
                package: None,
                version: None,
                source: Some("https://registry.npmjs.org".into()),
                command: "npm ci".into(),
                reason: format!(
                    "Runtime target {target_id} requires these locked dependencies: {}",
                    ids.join(", ")
                ),
                kind: DependencyKind::ExistingRestore,
                lockfile_identity: None,
                introduced_packages: Vec::new(),
            },
            PackageEcosystem::Cargo => DependencyNeed {
                ecosystem,
                package: None,
                version: None,
                source: Some("https://index.crates.io".into()),
                command: "cargo fetch --locked".into(),
                reason: format!(
                    "Runtime target {target_id} requires these locked dependencies: {}",
                    ids.join(", ")
                ),
                kind: DependencyKind::ExistingRestore,
                lockfile_identity: None,
                introduced_packages: Vec::new(),
            },
            _ => unreachable!(),
        };
        bridge.request_locked_dependency(need)?;
        prepared_dependency_ids.extend(packages.iter().map(|dependency| dependency.id.clone()));
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
