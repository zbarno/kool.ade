use super::super::support::*;
use super::qa_report;
use koolade::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{
        GateActor, GateDecision, GateReceipt, LifecycleStore, SpecReconciliationStatusV1,
        SpecRevisionDraftV1, TaskEvent,
    },
    harness::prompt_catalog::PromptRole,
    runtime_qa::{QaAssertionStatus, QaOutcome, RuntimeQaStore},
};

/// Production boundary: lifecycle reducers and the durable store atomically
/// complete a reviewed parent and bind its configured child to one PR item. A
/// unit test cannot prove the parent completion transaction includes the
/// persisted child exactly once. This uses temporary file-backed stores and
/// synthetic QA evidence, with low setup cost and no browser, network, Docker,
/// or provider dependency.
pub(super) fn complete_subtask(
    root: &std::path::Path,
    store: &PlanningStore,
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
    task_uid: &str,
) {
    let mut child = lifecycle.load_task(task_uid).unwrap();
    child = lifecycle
        .apply_task_event(task_uid, child.revision, &id(), TaskEvent::Start)
        .unwrap();
    let code_hash = "1".repeat(64);
    child = lifecycle
        .apply_task_event(
            task_uid,
            child.revision,
            &id(),
            TaskEvent::ImplementationFinished {
                code_sha256: code_hash.clone(),
            },
        )
        .unwrap();
    let receipt = role_receipt(
        PromptRole::CodeReviewer,
        GateDecision::Approved,
        task_uid,
        &code_hash,
    );
    child = lifecycle
        .apply_task_event(
            task_uid,
            child.revision,
            &id(),
            TaskEvent::CodeReview {
                receipt,
                reviewed_head_sha: "2".repeat(64),
                review_subject_sha256: "3".repeat(64),
            },
        )
        .unwrap();
    let (_request, outcome, report_receipt) =
        qa_report::execute_reviewed_run(root, lifecycle, &mut child, QaAssertionStatus::Passed);
    assert_eq!(outcome, QaOutcome::Pass);
    assert!(
        report_receipt.is_some(),
        "accepted QA run returns a review receipt"
    );
    child = lifecycle
        .record_runtime_qa_report(
            task_uid,
            child.revision,
            &id(),
            &RuntimeQaStore::new(root.join("qa-records")),
        )
        .unwrap();
    reconcile_no_change(
        store,
        lifecycle,
        feature_uid,
        "Synthetic child review found no specification delta for its persisted implementation evidence.",
    );
    let spec_receipt = GateReceipt {
        receipt_uid: id(),
        subject_uid: task_uid.to_owned(),
        subject_sha256: code_hash,
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 5,
        evidence_refs: vec!["fixture:child-spec-reconciliation/v1".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    lifecycle
        .apply_task_event(
            task_uid,
            child.revision,
            &id(),
            TaskEvent::SubtaskReconciled {
                receipt: spec_receipt,
            },
        )
        .unwrap();
}

pub(super) fn reconcile_no_change(
    store: &PlanningStore,
    lifecycle: &LifecycleStore<'_>,
    feature_uid: &str,
    reason: &str,
) {
    let pending_origins = lifecycle
        .list_spec_sync_queue(feature_uid, false)
        .unwrap()
        .into_iter()
        .map(|entry| entry.source_event_uid)
        .collect();
    let current_feature = lifecycle.load_feature(feature_uid).unwrap();
    let current_revision = lifecycle.load_spec_revision(feature_uid).unwrap().unwrap();
    lifecycle
        .append_spec_revision(
            feature_uid,
            current_revision.seq,
            SpecRevisionDraftV1 {
                content_hash: product_hash(store),
                source_revision: current_feature.revision,
                produced_by_run_id: None,
                origin_events: pending_origins,
                decision_refs: current_feature.decision_refs,
                delta_approval_refs: Vec::new(),
                approved_target_hash: current_feature.approved_plan_sha256,
                implementation_head_sha: None,
                reconciliation_status: SpecReconciliationStatusV1::Synchronized,
                outcome_reason: None,
                no_change_reason: Some(reason.to_owned()),
                deltas: Vec::new(),
            },
        )
        .unwrap();
}
