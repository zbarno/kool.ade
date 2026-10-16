use crate::app::session::Project;

use super::history;

pub(super) fn finish(
    project: &mut Project,
    task_uid: String,
    expected_revision: u64,
    spec_sha256: String,
    result: Result<crate::app::root::spec_reconciliation::SpecReconciliationOutcome, String>,
) {
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
    let task_is_current = match lifecycle.load_task(&task_uid) {
        Ok(task) => {
            task.revision == expected_revision
                && task.phase == crate::core::lifecycle::TaskPhase::Done
                && task.pr_work_item_uid.is_some()
        }
        Err(error) => {
            project.activity.pending.push(format!(
                "PR-triggered specification reconciliation for task {task_uid} could not verify its source task: {error}"
            ));
            return;
        }
    };
    if !task_is_current {
        project.activity.pending.push(format!(
            "PR-triggered specification reconciliation for task {task_uid} became stale; no result was recorded."
        ));
        return;
    }
    let current_hash = match crate::core::lifecycle::product_spec::current_product_hash(
        &project.state.planning_store,
    ) {
        Ok(hash) => hash,
        Err(error) => {
            project.activity.pending.push(format!(
                "PR-triggered specification reconciliation for task {task_uid} could not verify the current product specification: {error}"
            ));
            return;
        }
    };
    if current_hash != spec_sha256 {
        project.activity.pending.push(format!(
            "PR-triggered specification result for task {task_uid} is stale because the product specification changed; no result was recorded."
        ));
        return;
    }
    match result {
        Err(reason) => {
            record_result(project, &task_uid, None, Some(&reason), None);
            project.activity.pending.push(format!(
                "PR-triggered specification review for task {task_uid} needs attention: {reason}"
            ));
        }
        Ok(outcome)
            if outcome.task_uid == task_uid
                && outcome.expected_revision == expected_revision
                && outcome.spec_sha256 == spec_sha256 =>
        {
            let has_deltas = !outcome.proposal.deltas.is_empty();
            record_result(
                project,
                &task_uid,
                Some(&outcome.proposal),
                has_deltas.then_some("PR refresh found specification drift requiring human review."),
                Some(&outcome.run_uid),
            );
            if has_deltas {
                project.activity.pending.push(format!(
                    "PR-triggered reconciliation found possible specification drift for task {task_uid}; the proposal is recorded for human review. Provider-reported merge data was not treated as current implementation proof."
                ));
            }
        }
        Ok(_) => project.activity.pending.push(format!(
            "PR-triggered specification result for task {task_uid} is stale; no result was recorded."
        )),
    }
}

fn record_result(
    project: &mut Project,
    task_uid: &str,
    proposal: Option<&crate::harness::prompt_catalog::SpecDeltaV1>,
    reason: Option<&str>,
    run_uid: Option<&str>,
) {
    if let Err(error) = history::record(project, task_uid, proposal, reason, run_uid) {
        project.activity.pending.push(format!(
            "PR-triggered specification history for task {task_uid} could not be recorded: {error}"
        ));
    }
}
