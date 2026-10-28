use super::{Fixture, support};
use crate::core::lifecycle::{
    GateActor, GateDecision, GateReceipt, LifecycleStore, TaskEvent, TaskPhase,
};

pub(super) fn spec_receipt(task_uid: &str, code_hash: String) -> GateReceipt {
    GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: task_uid.to_owned(),
        subject_sha256: code_hash,
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 10,
        evidence_refs: vec!["spec/reconciled".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    }
}

/// Production boundary: the file-backed Done transition rejects unresolved
/// lifecycle triggers before task/PR writes; a task reducer unit test cannot
/// prove transactional queue gating. Synthetic temp store, moderate setup.
#[test]
fn task_done_transition_rejects_pending_spec_triggers() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, task_uid, _, _) = support::create_feature_graph(&lifecycle);
    let (task, _) = support::finish_task_through_qa(&fixture, &lifecycle, &task_uid);
    let manual = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "done-gate-pending")
        .unwrap();
    let error = lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::ReconcileSpec {
                receipt: spec_receipt(&task_uid, task.implementation_sha256.unwrap()),
                pr_work_item_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("unresolved lifecycle trigger"));
    assert_eq!(
        lifecycle.load_task(&task_uid).unwrap().phase,
        TaskPhase::AwaitingSpecReconciliation
    );
    assert!(
        lifecycle
            .list_spec_sync_queue(&feature_uid, false)
            .unwrap()
            .iter()
            .any(|entry| entry.uid == manual.uid)
    );
}

/// A product-document edit after the last durable revision blocks task
/// completion even when its QA and review receipts are still valid.
#[test]
fn task_done_transition_rejects_product_spec_drift() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (_, task_uid, _, _) = support::create_feature_graph(&lifecycle);
    let (task, _) = support::finish_task_through_qa(&fixture, &lifecycle, &task_uid);
    let documents = crate::artifacts::product_docs::load_documents(&store)
        .unwrap()
        .expect("baseline product specification");
    let document = &documents[0];
    let relative_path = store
        .layout()
        .product_root()
        .join(&document.module.path)
        .strip_prefix(store.layout().root())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    store
        .transaction(
            &[(
                relative_path,
                format!("{}\n\nA deliberate product change.\n", document.content).into_bytes(),
            )],
            None,
        )
        .unwrap();
    let error = lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::ReconcileSpec {
                receipt: spec_receipt(&task_uid, task.implementation_sha256.unwrap()),
                pr_work_item_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            },
        )
        .unwrap_err();
    assert!(error.to_string().contains("drifted"));
    assert_eq!(
        lifecycle.load_task(&task_uid).unwrap().phase,
        TaskPhase::AwaitingSpecReconciliation
    );
}

/// Production boundary: a destination integration that moves the exact parent
/// head after Done wipes every completion receipt and drops the task to
/// ReReview, and the stale code-review / QA receipts bound to the previous
/// implementation SHA are rejected when replayed — enforcing the protocol rule
/// "once code changes, invalidate relevant QA and code review receipts" so
/// repair -> re-review -> re-QA is mandatory. Receipt eligibility spans task
/// transitions, validation, and file-backed store transactions, which a
/// reducer-only unit assertion cannot prove. No external services; uses a
/// synthetic temporary file store. Moderate setup cost (full completion
/// fixture).
#[test]
fn code_head_change_after_qa_pass_invalidates_stale_receipts() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (_, task_uid, _, _) = support::create_feature_graph(&lifecycle);
    let (task, old_qa_request) = support::finish_task_through_qa(&fixture, &lifecycle, &task_uid);
    let old_code_sha = task.implementation_sha256.clone().unwrap();
    let mut stale_review_receipt = task.code_review.clone().unwrap();
    let mut stale_qa_receipt = task.runtime_qa.clone().unwrap();
    assert_eq!(stale_review_receipt.subject_sha256, old_code_sha);
    assert_eq!(stale_qa_receipt.subject_sha256, old_code_sha);

    let done = lifecycle
        .apply_task_event(
            &task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::ReconcileSpec {
                receipt: spec_receipt(&task_uid, old_code_sha.clone()),
                pr_work_item_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            },
        )
        .unwrap();
    assert_eq!(done.phase, TaskPhase::Done);

    // Application-controlled destination integration observes a newer head.
    let invalidated = lifecycle
        .apply_task_event(
            &task_uid,
            done.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::PublicationHeadChanged {
                code_sha256: "d".repeat(64),
                head_sha: "b".repeat(40),
            },
        )
        .unwrap();
    assert_eq!(invalidated.phase, TaskPhase::ReReview);
    assert_eq!(
        invalidated.implementation_sha256.as_deref(),
        Some("d".repeat(64).as_str())
    );
    assert!(invalidated.code_review.is_none());
    assert!(invalidated.runtime_qa.is_none());
    assert!(invalidated.qa_execution.is_none());
    assert!(invalidated.spec_reconciliation.is_none());

    // Use a fresh receipt ID and current head so the old subject hash is the
    // only stale-review condition under test.
    stale_review_receipt.receipt_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    assert!(stale_review_receipt.validate().is_ok());
    let stale_review_error = lifecycle
        .apply_task_event(
            &task_uid,
            invalidated.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::CodeReview {
                receipt: stale_review_receipt,
                reviewed_head_sha: "b".repeat(40),
                review_subject_sha256: "2".repeat(64),
            },
        )
        .unwrap_err();
    assert_ne!(stale_review_error.to_string(), "");
    let after_stale_review = lifecycle.load_task(&task_uid).unwrap();
    assert_eq!(after_stale_review.phase, TaskPhase::ReReview);
    assert!(after_stale_review.code_review.is_none());

    // Fresh review of the NEW head is accepted...
    let mut fresh_review = super::super::super::receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &task_uid,
    );
    fresh_review.subject_sha256 = "d".repeat(64);
    let re_reviewed = lifecycle
        .apply_task_event(
            &task_uid,
            after_stale_review.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::CodeReview {
                receipt: fresh_review,
                reviewed_head_sha: "b".repeat(40),
                review_subject_sha256: "2".repeat(64),
            },
        )
        .unwrap();
    assert_eq!(re_reviewed.phase, TaskPhase::AwaitingRuntimeQa);

    // ...and a QA request is re-queued against the NEW head.
    let fresh_qa_request = super::super::super::qa_request(&re_reviewed);
    let fresh_qa_run_id = fresh_qa_request.qa_run_id.clone();
    let fresh_author_run_id = fresh_qa_request.author_run_id.clone();
    let fresh_author_session_id = fresh_qa_request.author_session_id.clone();
    assert_ne!(fresh_qa_run_id, old_qa_request.qa_run_id);
    let re_queued = lifecycle
        .queue_runtime_qa(
            &task_uid,
            re_reviewed.revision,
            &uuid::Uuid::new_v4().to_string(),
            fresh_qa_request,
        )
        .unwrap();
    assert_eq!(re_queued.phase, TaskPhase::AwaitingRuntimeQa);

    // Align every receipt binding except subject_sha256 with the current QA
    // request, isolating the stale implementation hash as the rejection cause.
    let replay_reviewer_execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let replay_reviewer_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let result_sha256 = "3".repeat(64);
    let evidence_manifest_sha256 = "4".repeat(64);
    stale_qa_receipt.receipt_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    stale_qa_receipt.reviewer_run_id = Some(replay_reviewer_execution_id.clone());
    stale_qa_receipt.reviewer_session_id = Some(replay_reviewer_session_id.clone());
    stale_qa_receipt.author_run_id = Some(fresh_author_run_id);
    stale_qa_receipt.author_session_id = Some(fresh_author_session_id);
    stale_qa_receipt.evidence_refs = vec![
        format!("runtime-qa:{fresh_qa_run_id}/result"),
        format!("runtime-qa:{fresh_qa_run_id}/evidence:{evidence_manifest_sha256}"),
    ];
    assert!(stale_qa_receipt.validate().is_ok());
    let stale_qa_error = lifecycle
        .apply_task_event(
            &task_uid,
            re_queued.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::RuntimeQaCompleted {
                request_id: fresh_qa_run_id,
                receipt: stale_qa_receipt,
                result_sha256,
                evidence_manifest_sha256,
                reviewer_execution_id: replay_reviewer_execution_id,
                reviewer_session_id: replay_reviewer_session_id,
            },
        )
        .unwrap_err();
    assert_ne!(stale_qa_error.to_string(), "");
    let final_task = lifecycle.load_task(&task_uid).unwrap();
    assert_eq!(final_task.phase, TaskPhase::AwaitingRuntimeQa);
    assert!(final_task.runtime_qa.is_none());
    assert!(final_task.qa_execution.as_ref().is_some_and(|state| {
        matches!(
            state.status,
            crate::core::lifecycle::QaExecutionStatusV1::Pending
                | crate::core::lifecycle::QaExecutionStatusV1::Running
        )
    }));
}
