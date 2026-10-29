use crate::{
    core::lifecycle::TaskRecord,
    execution::{DependencyCacheSnapshotStore, contract::DependencyCacheRefV1},
    project_runtime::AppTargetV1,
};
use std::collections::BTreeSet;

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
    crate::app::root::runtime_dependencies::prepared_broker_dependencies(task, project_id, progress)
        .into_iter()
        .filter(|dependency| {
            target
                .required_dependencies
                .contains(&dependency.request_id)
        })
        .flat_map(|dependency| {
            let request_id = dependency.request_id;
            dependency
                .cache_refs
                .into_iter()
                .map(move |cache_ref| (request_id.clone(), cache_ref))
        })
        .collect()
}
