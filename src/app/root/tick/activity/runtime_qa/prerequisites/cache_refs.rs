use crate::{
    core::lifecycle::TaskRecord,
    execution::{DependencyCacheSnapshotStore, contract::DependencyCacheRefV1},
    project_runtime::AppTargetV1,
};
use std::collections::{BTreeMap, BTreeSet};

pub(in crate::app::root::tick::activity::runtime_qa) fn available_broker_dependencies(
    target: &AppTargetV1,
    task: &TaskRecord,
    project_id: &str,
    progress: Option<&crate::harness::LiveProgress>,
    store: &DependencyCacheSnapshotStore,
) -> BTreeSet<String> {
    selected_broker_cache_refs(target, task, project_id, progress)
        .into_iter()
        .filter(|(request_id, cache_ref)| store.get(request_id, cache_ref).is_ok())
        .map(|(request_id, _)| request_id)
        .collect()
}

pub(in crate::app::root::tick::activity::runtime_qa) fn selected_broker_cache_refs(
    target: &AppTargetV1,
    task: &TaskRecord,
    project_id: &str,
    progress: Option<&crate::harness::LiveProgress>,
) -> Vec<(String, DependencyCacheRefV1)> {
    let Some(progress) = progress else {
        return Vec::new();
    };
    let mut selected = BTreeMap::new();
    for request in &progress.dependency_requests {
        if !target.required_dependencies.contains(&request.id)
            || !(request.task_id == task.task_uid
                || task.planning_item_uid.as_deref() == Some(request.task_id.as_str()))
            || request.status != crate::harness::DependencyRequestStatus::Prepared
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
                selected.insert(request.id.clone(), cache_ref.clone());
            }
        }
    }
    selected.into_iter().collect()
}

fn valid_cache_ref(
    request: &crate::harness::DependencyRequest,
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
