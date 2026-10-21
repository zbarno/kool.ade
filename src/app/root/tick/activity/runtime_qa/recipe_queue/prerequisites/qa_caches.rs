use super::{
    DependencyKind, DependencyNeed, PackageEcosystem, dependencies::parse_required_dependencies,
    safe_target_root,
};
use crate::execution::session::ExecutionSession;
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool, mpsc::Sender},
};

pub(crate) fn prepare_qa_dependency_caches(
    session: Arc<ExecutionSession>,
    clone_path: &Path,
    project_relative_root: &str,
    target_id: &str,
    required_dependencies: &[String],
    cancel: Arc<AtomicBool>,
    progress: Sender<crate::harness::LiveProgress>,
) -> Result<Option<crate::harness::resource_bridge::ResourceBridge>, String> {
    let dependencies = parse_required_dependencies(required_dependencies)?;
    let packages = dependencies
        .iter()
        .filter(|dependency| {
            matches!(
                dependency.ecosystem,
                PackageEcosystem::Npm | PackageEcosystem::Cargo
            )
        })
        .collect::<Vec<_>>();
    if packages.is_empty() {
        return Ok(None);
    }
    let target_root = safe_target_root(clone_path, project_relative_root)?;
    let bridge = crate::harness::resource_bridge::ResourceBridge::start_for_task_repository_target(
        clone_path,
        &target_root,
        None,
        Some(&session.workspace().task_id),
        progress,
        cancel,
    )
    .map_err(|error| format!("task dependency broker could not start: {error:#}"))?;
    bridge
        .bind_execution_session(Arc::clone(&session))
        .map_err(|error| {
            format!("task dependency broker could not bind the QA session: {error:#}")
        })?;
    for ecosystem in [PackageEcosystem::Npm, PackageEcosystem::Cargo] {
        let selected = packages
            .iter()
            .filter(|dependency| dependency.ecosystem == ecosystem)
            .collect::<Vec<_>>();
        if selected.is_empty() {
            continue;
        }
        for dependency in &selected {
            if !crate::harness::resource_bridge::has_locked_package(
                &target_root,
                ecosystem,
                &dependency.package,
            )
            .map_err(|error| {
                format!(
                    "could not verify locked target dependency {}: {error:#}",
                    dependency.id
                )
            })? {
                return Err(format!(
                    "target dependency {} is absent from the target's verified lockfile",
                    dependency.id
                ));
            }
        }
        request_locked_cache(&bridge, &session, ecosystem, target_id, &selected)?;
    }
    Ok(Some(bridge))
}

fn request_locked_cache(
    bridge: &crate::harness::resource_bridge::ResourceBridge,
    session: &ExecutionSession,
    ecosystem: PackageEcosystem,
    target_id: &str,
    selected: &[&&super::dependencies::RequiredDependency],
) -> Result<(), String> {
    let ids = selected
        .iter()
        .map(|dependency| dependency.id.as_str())
        .collect::<Vec<_>>();
    let (source, command) = match ecosystem {
        PackageEcosystem::Npm => ("https://registry.npmjs.org", "npm ci"),
        PackageEcosystem::Cargo => ("https://index.crates.io", "cargo fetch --locked"),
        _ => return Err("unsupported QA package cache ecosystem".into()),
    };
    let need = DependencyNeed {
        ecosystem,
        package: None,
        version: None,
        source: Some(source.into()),
        command: command.into(),
        reason: format!(
            "Runtime target {target_id} requires these locked dependencies: {}",
            ids.join(", ")
        ),
        kind: DependencyKind::ExistingRestore,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    };
    bridge.request_locked_dependency(need)?;
    session
        .record_prepared_dependency_ids(
            &selected
                .iter()
                .map(|dependency| dependency.id.clone())
                .collect::<Vec<_>>(),
        )
        .map_err(|error| {
            format!(
                "QA dependency evidence could not be bound to the session: {}",
                error.message
            )
        })
}
