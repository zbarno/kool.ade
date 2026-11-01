use crate::{
    app::session::Project,
    core::lifecycle::{LifecycleStore, TaskRecord},
    runtime_qa::{QaRunRequestV1, RuntimeQaStore},
};

pub(super) fn record(
    project: &mut Project,
    task: &TaskRecord,
    request: &QaRunRequestV1,
    store: &RuntimeQaStore,
) -> Result<Option<String>, String> {
    let result = super::super::reports::record_persisted_report(
        &project.state.planning_store,
        &mut project.planning_work,
        &mut project.state.baseline_planning_revision,
        &LifecycleStore::new(&project.state.planning_store),
        store,
        task,
    )?;
    if result.is_some() {
        finish_present(project, task, request, store);
    }
    Ok(result)
}

pub(super) fn finish_present(
    project: &mut Project,
    task: &TaskRecord,
    expected_request: &QaRunRequestV1,
    store: &RuntimeQaStore,
) {
    if let Ok((request, _, _, result)) = store.load_current_run(&expected_request.qa_run_id)
        && request == *expected_request
    {
        crate::app::manager::finish_qa_finding_repair(project, &task.task_uid, &request, &result);
    }
}
