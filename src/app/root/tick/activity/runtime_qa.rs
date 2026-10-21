use crate::{app::session::Project, core::lifecycle::LifecycleStore};
use prerequisites::{event_uid, unavailable_reason};

mod advance;
mod prerequisites;
pub(crate) mod recipe_queue;
mod reports;
mod request;
mod worker;

/// Reconciles durable QA requests on every project tick. Missing trusted
/// runtime assets become persisted blockers; this path never invents test
/// commands, reviewer provenance, or a passing result.
pub(super) use advance::advance_runtime_qa;

pub(crate) fn validate_request_fresh(
    project: &Project,
    task_uid: &str,
    expected: &crate::runtime_qa::QaRunRequestV1,
) -> Result<(), String> {
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle
        .load_task(task_uid)
        .map_err(|error| error.to_string())?;
    let prepared = request::prepare(project, &task)?;
    if prepared.request != *expected {
        return Err("Runtime QA request inputs changed; generate a fresh proposal for the current reviewed build.".into());
    }
    Ok(())
}

pub(crate) fn scenario_source_evidence(
    project: &Project,
    task: &crate::core::lifecycle::TaskRecord,
    expected: &crate::runtime_qa::QaRunRequestV1,
) -> Result<crate::runtime_qa::scenario_synthesis::QaScenarioSourceEvidenceV1, String> {
    let prepared = request::prepare(project, task)?;
    if prepared.request != *expected {
        return Err("Runtime QA request changed while source evidence was collected.".into());
    }
    request::scenario_synthesis_evidence(project, task, &prepared)
}

pub(crate) fn persist_synthesized_proposal(
    project: &Project,
    request_snapshot: &crate::runtime_qa::QaRunRequestV1,
    proposal: &crate::runtime_qa::QaScenarioProposalV1,
) -> Result<(), String> {
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let task = lifecycle
        .load_task(&request_snapshot.parent_task_uid)
        .map_err(|error| error.to_string())?;
    let current = request::prepare(project, &task)?.request;
    let evidence = scenario_source_evidence(project, &task, request_snapshot)?;
    let store = crate::runtime_qa::RuntimeQaStore::new(
        crate::runtime_qa::RuntimeQaStore::local_root_for_workspace(
            project.state.planning_store.project_id,
        )
        .map_err(|error| format!("local runtime QA evidence storage is unavailable: {error}"))?,
    );
    crate::runtime_qa::scenario_synthesis::persist_proposal_if_fresh(
        &store,
        request_snapshot,
        proposal,
        &evidence,
        &current,
        task.revision,
    )
}
