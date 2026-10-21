use crate::{
    core::lifecycle::TaskRecord,
    execution::{DependencyCacheSnapshotStore, contract::DependencyCacheRefV1},
    harness::{DependencyRequest, DependencyRequestStatus, LiveProgress},
};
use std::collections::BTreeMap;

#[derive(Clone)]
pub(in crate::app::root) struct PreparedBrokerDependency {
    pub(in crate::app::root) request_id: String,
    pub(in crate::app::root) label: String,
    pub(in crate::app::root) cache_ref: DependencyCacheRefV1,
}

/// Returns only same-task broker preparations whose retained cache snapshot
/// still matches its task, project, ecosystem, and lockfile identity.
pub(in crate::app::root) fn prepared_broker_dependencies(
    task: &TaskRecord,
    project_id: &str,
    progress: Option<&LiveProgress>,
) -> Vec<PreparedBrokerDependency> {
    let Some(progress) = progress else {
        return Vec::new();
    };
    let mut selected = BTreeMap::new();
    for request in &progress.dependency_requests {
        if !matches!(request.status, DependencyRequestStatus::Prepared)
            || !(request.task_id == task.task_uid
                || task.planning_item_uid.as_deref() == Some(request.task_id.as_str()))
        {
            continue;
        }
        let Some(preparation) = request.preparation.as_ref().filter(|preparation| {
            preparation.status == Some(crate::harness::DependencyPreparationStatus::Prepared)
        }) else {
            continue;
        };
        let Some(lockfile_identity) = request.need.lockfile_identity.as_deref() else {
            continue;
        };
        for cache_ref in &preparation.cache_refs {
            if valid_cache_ref(request, task, project_id, lockfile_identity, cache_ref) {
                selected.insert(
                    request.id.clone(),
                    PreparedBrokerDependency {
                        request_id: request.id.clone(),
                        label: dependency_label(request),
                        cache_ref: cache_ref.clone(),
                    },
                );
            }
        }
    }
    selected.into_values().collect()
}

fn valid_cache_ref(
    request: &DependencyRequest,
    task: &TaskRecord,
    project_id: &str,
    lockfile_identity: &str,
    cache_ref: &DependencyCacheRefV1,
) -> bool {
    let ecosystem = match request.need.ecosystem {
        crate::harness::PackageEcosystem::Npm => {
            crate::execution::contract::DependencyCacheEcosystemV1::Npm
        }
        crate::harness::PackageEcosystem::Cargo => {
            crate::execution::contract::DependencyCacheEcosystemV1::Cargo
        }
        _ => return false,
    };
    cache_ref.validate().is_ok()
        && task.planning_item_uid.as_deref() == Some(request.task_id.as_str())
        && cache_ref.task_id == request.task_id
        && cache_ref.project_id == project_id
        && cache_ref.target_id == "local"
        && cache_ref.resolved_integrity == lockfile_identity
        && cache_ref.ecosystem == ecosystem
}

fn dependency_label(request: &DependencyRequest) -> String {
    let ecosystem = match request.need.ecosystem {
        crate::harness::PackageEcosystem::Npm => "npm",
        crate::harness::PackageEcosystem::Pnpm => "pnpm",
        crate::harness::PackageEcosystem::Yarn => "Yarn",
        crate::harness::PackageEcosystem::Cargo => "Cargo",
        crate::harness::PackageEcosystem::Nuget => "NuGet",
        crate::harness::PackageEcosystem::Pip => "pip",
        crate::harness::PackageEcosystem::Uv => "uv",
        crate::harness::PackageEcosystem::Poetry => "Poetry",
        crate::harness::PackageEcosystem::System => "system",
        crate::harness::PackageEcosystem::Other => "other",
    };
    let package = request
        .need
        .package
        .as_deref()
        .unwrap_or("prepared package set");
    let safe_name = safe_label_value(package, 80);
    let version = request
        .need
        .version
        .as_deref()
        .map(|version| safe_label_value(version, 40))
        .filter(|version| !version.is_empty())
        .map(|version| format!("@{version}"))
        .unwrap_or_default();
    format!("{ecosystem}: {safe_name}{version}")
}

fn safe_label_value(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect()
}

/// Load the prepared dependency options shown for a task target. A failed
/// private cache read fails closed by omitting that option.
pub(in crate::app::root) fn prepared_dependencies_for_ticket(
    project: &crate::app::session::Project,
    ticket: &str,
) -> Vec<PreparedBrokerDependency> {
    let Some(document) = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
    else {
        return Vec::new();
    };
    let Some(task_id) = document
        .metadata
        .as_ref()
        .map(|metadata| metadata.uid.as_str())
    else {
        return Vec::new();
    };
    let Ok(task) = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store)
        .load_task(task_id)
    else {
        return Vec::new();
    };
    let project_id = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let progress = project.activity.tasks.get(ticket);
    let candidates = prepared_broker_dependencies(&task, &project_id, progress);
    let store = DependencyCacheSnapshotStore::new(
        crate::persistence::state_root().join("runtime-qa-dependency-caches"),
    );
    candidates
        .into_iter()
        .filter(|candidate| {
            store
                .get(&candidate.request_id, &candidate.cache_ref)
                .is_ok()
        })
        .collect()
}

#[cfg(test)]
#[path = "runtime_dependencies/tests.rs"]
mod tests;
