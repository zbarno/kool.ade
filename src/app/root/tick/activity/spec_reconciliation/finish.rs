use super::{event_uid, hash_text, now_ms, stable_uid};
use crate::harness::prompt_catalog::SpecDeltaV1;
use crate::{
    app::session::Project,
    core::lifecycle::{GateActor, GateDecision, GateReceipt, LifecycleStore},
};
use sha2::Digest as _;

pub(super) fn finish(
    project: &mut Project,
    task_uid: String,
    expected_revision: u64,
    spec_sha256: String,
    result: Result<crate::app::root::spec_reconciliation::SpecReconciliationOutcome, String>,
) {
    project.live_progress = Default::default();
    let current_spec_sha256 = crate::app::root::spec_reconciliation::spec_binding_hash(
        project.state.spec_text.as_deref(),
    )
    .unwrap_or_default();
    match result {
        Err(reason) => {
            record_blocker(
                project,
                &task_uid,
                expected_revision,
                &spec_sha256,
                hash_text(&reason),
                format!("SpecReconciler did not complete: {reason}"),
                None,
            );
        }
        Ok(outcome) => {
            let proposal_sha256 = hash_json(&outcome.proposal)
                .unwrap_or_else(|| hash_text("invalid proposal serialization"));
            if outcome.task_uid != task_uid
                || outcome.expected_revision != expected_revision
                || outcome.spec_sha256 != spec_sha256
                || current_spec_sha256 != spec_sha256
            {
                record_blocker(
                    project,
                    &task_uid,
                    expected_revision,
                    &spec_sha256,
                    proposal_sha256,
                    "Current task or specification changed while reconciliation was running; rerun against the latest exact state.".into(),
                    None,
                );
                return;
            }
            if !outcome.proposal.deltas.is_empty() {
                let reason = delta_summary(&outcome.proposal);
                record_blocker(
                    project,
                    &task_uid,
                    expected_revision,
                    &spec_sha256,
                    proposal_sha256,
                    reason,
                    serde_json::to_string(&outcome.proposal).ok(),
                );
                return;
            }
            let lifecycle = LifecycleStore::new(&project.state.planning_store);
            let task = match lifecycle.load_task(&task_uid) {
                Ok(task) if task.revision == expected_revision => task,
                Ok(_) => {
                    project.activity.pending.push(format!(
                        "SpecReconciler result for task {task_uid} is stale; no completion was recorded."
                    ));
                    return;
                }
                Err(error) => {
                    project.activity.pending.push(format!(
                        "SpecReconciler result for task {task_uid} could not be checked: {error}"
                    ));
                    return;
                }
            };
            let Some(review) = task.code_review.as_ref() else {
                project.activity.pending.push(format!(
                    "SpecReconciler result for task {task_uid} has no current code-review receipt."
                ));
                return;
            };
            let Some(qa_state) = task.qa_execution.as_ref() else {
                project.activity.pending.push(format!(
                    "SpecReconciler result for task {task_uid} has no current QA record."
                ));
                return;
            };
            let Some(request) = qa_state.request.as_ref() else {
                project.activity.pending.push(format!(
                    "SpecReconciler result for task {task_uid} has no exact QA request."
                ));
                return;
            };
            let Some(result_hash) = qa_state.result_sha256.as_deref() else {
                project.activity.pending.push(format!(
                    "SpecReconciler result for task {task_uid} has no persisted QA result hash."
                ));
                return;
            };
            let Some(evidence_hash) = qa_state.evidence_manifest_sha256.as_deref() else {
                project.activity.pending.push(format!(
                    "SpecReconciler result for task {task_uid} has no persisted QA evidence hash."
                ));
                return;
            };
            let Some(code_sha256) = task.implementation_sha256.as_deref() else {
                project.activity.pending.push(format!(
                    "SpecReconciler result for task {task_uid} has no reviewed diff identity."
                ));
                return;
            };
            let receipt_uid = stable_uid(
                &task_uid,
                &format!("spec-reconcile-receipt:v1:{spec_sha256}:{proposal_sha256}"),
            );
            let pr_work_uid = stable_uid(
                &task_uid,
                &format!(
                    "lifecycle-pr-work:v1:{}:{code_sha256}",
                    task.approved_plan_sha256
                ),
            );
            let receipt = GateReceipt {
                receipt_uid,
                subject_uid: task_uid.clone(),
                subject_sha256: code_sha256.to_owned(),
                actor: GateActor::ApplicationPolicy,
                decision: GateDecision::Approved,
                recorded_at_unix_ms: now_ms(),
                evidence_refs: vec![
                    format!("plan:{}", task.approved_plan_sha256),
                    format!("code-review:{}", review.receipt_uid),
                    format!(
                        "code-review-subject:{}",
                        task.reviewed_code_subject_sha256
                            .as_deref()
                            .unwrap_or_default()
                    ),
                    format!("runtime-qa:{}/result:{result_hash}", request.qa_run_id),
                    format!("runtime-qa:{}/evidence:{evidence_hash}", request.qa_run_id),
                    format!("spec-reconciler:{}", outcome.prompt.identity_sha256()),
                    format!("current-spec:{spec_sha256}"),
                ],
                prompt_sha256: None,
                prompt_manifest: None,
                reviewer_run_id: None,
                author_run_id: None,
                reviewer_session_id: None,
                author_session_id: None,
            };
            let event_uid = event_uid(
                &task_uid,
                expected_revision,
                "spec-reconciled-v1",
                &proposal_sha256,
            );
            let reconciled = if task.parent_task_uid.is_some() {
                lifecycle.reconcile_subtask_specification(
                    &task_uid,
                    expected_revision,
                    &event_uid,
                    receipt,
                )
            } else {
                lifecycle.reconcile_specification(
                    &task_uid,
                    expected_revision,
                    &event_uid,
                    receipt,
                    &pr_work_uid,
                )
            };
            match reconciled {
                Ok(_) => {
                    let status = if task.parent_task_uid.is_some() {
                        "The subtask is Done and remains included in its parent task’s pull request."
                    } else {
                        "The standalone task is Done; lifecycle PR work is awaiting explicit authorization."
                    };
                    let message = format!(
                        "Specification reconciliation passed for task {}: no approved requirement delta was needed. {status}",
                        task.planning_item_uid.as_deref().unwrap_or(&task_uid)
                    );
                    project.activity.pending.push(message.clone());
                    project.remember_chat(vec![crate::domain::chatlog::ChatMessage::new(
                        crate::domain::chatlog::ChatRole::System,
                        message,
                        None,
                    )]);
                }
                Err(error) => project.activity.pending.push(format!(
                    "Specification reconciliation for task {task_uid} could not be committed: {error}"
                )),
            }
        }
    }
}

pub(super) fn record_blocker(
    project: &mut Project,
    task_uid: &str,
    expected_revision: u64,
    spec_sha256: &str,
    proposal_sha256: String,
    reason: String,
    proposal_json: Option<String>,
) {
    let lifecycle = LifecycleStore::new(&project.state.planning_store);
    let event_uid = event_uid(
        task_uid,
        expected_revision,
        "spec-reconciliation-blocked-v1",
        &proposal_sha256,
    );
    match lifecycle.block_spec_reconciliation(
        task_uid,
        expected_revision,
        &event_uid,
        crate::core::lifecycle::SpecReconciliationBlockV1 {
            reason: reason.clone(),
            proposal_sha256,
            spec_sha256: spec_sha256.to_owned(),
            proposal_json,
        },
    ) {
        Ok(_) => {
            let notice = format!(
                "Specification reconciliation for task {task_uid} is blocked: {reason} The approved specification was not changed; resolve the decision and update the specification before retrying."
            );
            project.activity.pending.push(notice.clone());
            project.remember_chat(vec![crate::domain::chatlog::ChatMessage::new(
                crate::domain::chatlog::ChatRole::System,
                notice,
                None,
            )]);
        }
        Err(error) => project.activity.pending.push(format!(
            "Specification reconciliation blocker for task {task_uid} could not be persisted: {error}"
        )),
    }
}

fn delta_summary(proposal: &SpecDeltaV1) -> String {
    let details = proposal
        .deltas
        .iter()
        .take(16)
        .map(|delta| {
            format!(
                "{}: {} → {} ({:?}, approval={:?})",
                delta.requirement_id,
                crate::core::context_build::clip(&delta.old, 200),
                crate::core::context_build::clip(&delta.new, 200),
                delta.classification,
                delta.approval_effect
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "SpecReconciler proposed {} requirement delta(s): {details}",
        proposal.deltas.len()
    )
}

fn hash_json(value: &impl serde::Serialize) -> Option<String> {
    serde_json::to_vec(value)
        .ok()
        .map(|value| format!("{:x}", sha2::Sha256::digest(value)))
}
