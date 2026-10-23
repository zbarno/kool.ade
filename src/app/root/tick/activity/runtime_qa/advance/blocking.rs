use super::super::{event_uid, worker};
use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{LifecycleStore, QaExecutionStatusV1, TaskPhase, TaskRecord},
    runtime_qa::{QaRunRequestV1, RuntimeQaStore},
};
use std::path::Path;

pub(super) enum BlockOutcome {
    Persisted,
    ActiveWorker,
    Stale,
    ReportPresent(String),
}

/// Persist a blocker only while the observed request still owns the lifecycle
/// snapshot. Running requests also require the per-run lock, so a second app
/// cannot block a worker that is still producing its report.
pub(super) fn persist_block(
    planning_store: &PlanningStore,
    qa_root: &Path,
    snapshot: &TaskRecord,
    request: &QaRunRequestV1,
    event_kind: &str,
    reason: String,
) -> Result<BlockOutcome, String> {
    let lifecycle = LifecycleStore::new(planning_store);
    let current = lifecycle
        .load_task(&snapshot.task_uid)
        .map_err(|error| error.to_string())?;
    if !matches_snapshot(&current, snapshot, request) {
        return Ok(BlockOutcome::Stale);
    }

    let _run_lock = if current
        .qa_execution
        .as_ref()
        .is_some_and(|execution| execution.status == QaExecutionStatusV1::Running)
    {
        let lock_path = qa_root.join(&request.qa_run_id).join("worker.lock");
        match worker::acquire_run_lock(&lock_path) {
            Ok(lock) => Some(lock),
            Err(worker::RunLockError::ActiveWorker) => return Ok(BlockOutcome::ActiveWorker),
            Err(worker::RunLockError::Failed(error)) => return Err(error),
        }
    } else {
        None
    };

    let current = lifecycle
        .load_task(&snapshot.task_uid)
        .map_err(|error| error.to_string())?;
    if !matches_snapshot(&current, snapshot, request) {
        return Ok(BlockOutcome::Stale);
    }
    if _run_lock.is_some() {
        let qa_store = RuntimeQaStore::new(qa_root);
        let running_restart = current
            .qa_execution
            .as_ref()
            .is_some_and(|execution| execution.status == QaExecutionStatusV1::Running);
        if super::super::reports::persisted_report_state(&qa_store, request, running_restart)
            == super::super::reports::PersistedReportState::Recordable
        {
            let notice = match super::super::reports::record_persisted_report(
                planning_store,
                &lifecycle,
                &qa_store,
                &current,
            ) {
                Ok(Some(notice)) => notice,
                Ok(None) => "A completed Runtime QA report was preserved for the next tick.".into(),
                Err(error) => format!(
                    "A completed Runtime QA report was preserved and could not yet be applied: {error}"
                ),
            };
            return Ok(BlockOutcome::ReportPresent(notice));
        }
    }
    let id = event_uid(&current.task_uid, current.revision, event_kind);
    lifecycle
        .block_runtime_qa(
            &current.task_uid,
            current.revision,
            &id,
            &request.qa_run_id,
            reason,
        )
        .map_err(|error| error.to_string())?;
    Ok(BlockOutcome::Persisted)
}

fn matches_snapshot(current: &TaskRecord, snapshot: &TaskRecord, request: &QaRunRequestV1) -> bool {
    current.revision == snapshot.revision
        && current.phase == TaskPhase::AwaitingRuntimeQa
        && current.qa_execution.as_ref().is_some_and(|execution| {
            matches!(
                execution.status,
                QaExecutionStatusV1::Pending | QaExecutionStatusV1::Running
            ) && execution.request.as_ref() == Some(request)
        })
}
