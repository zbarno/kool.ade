use super::*;
use sha2::Digest as _;

#[test]
fn stale_or_same_context_review_cannot_advance_task_to_done() {
    let task_uid = id();
    let feature_uid = id();
    let code_sha = "c".repeat(64);
    let mut task = TaskRecord {
        schema_version: 1,
        task_uid: task_uid.clone(),
        feature_uid,
        planning_item_uid: None,
        task_spec_sha256: None,
        parent_task_uid: Some(id()),
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: Vec::new(),
        phase: TaskPhase::Ready,
        revision: 0,
        approved_plan_sha256: "b".repeat(64),
        implementation_sha256: None,
        reviewed_code_head_sha: None,
        reviewed_code_subject_sha256: None,
        code_review: None,
        review_history: Vec::new(),
        repair_attempts: 0,
        runtime_qa: None,
        qa_execution: None,
        spec_reconciliation: None,
        spec_reconciliation_blocker: None,
        spec_reconciliation_proposal_sha256: None,
        spec_reconciliation_spec_sha256: None,
        spec_reconciliation_proposal_json: None,
        pr_work_item_uid: None,
    };
    let mut legacy = serde_json::to_value(&task).unwrap();
    legacy
        .as_object_mut()
        .unwrap()
        .remove("approved_requirement_ids");
    let legacy: TaskRecord = serde_json::from_value(legacy).unwrap();
    assert!(legacy.approved_requirement_ids.is_empty());

    task.apply(0, TaskEvent::Start).unwrap();
    task.apply(
        1,
        TaskEvent::ImplementationFinished {
            code_sha256: code_sha.clone(),
        },
    )
    .unwrap();
    let mut review = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &task_uid,
    );
    review.subject_sha256 = "d".repeat(64);
    assert!(task.apply(2, code_review_event(review)).is_err());

    let mut review = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &task_uid,
    );
    review.subject_sha256 = code_sha.clone();
    review.reviewer_run_id = review.author_run_id.clone();
    assert!(task.apply(2, code_review_event(review)).is_err());

    let mut review = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &task_uid,
    );
    review.subject_sha256 = code_sha.clone();
    task.apply(2, code_review_event(review)).unwrap();
    task.apply(
        task.revision,
        TaskEvent::RuntimeQaUnavailable {
            reason: "No approved startup recipe is available".into(),
        },
    )
    .unwrap();
    let blocked_qa = task.qa_execution.as_ref().unwrap();
    assert_eq!(blocked_qa.status, QaExecutionStatusV1::Blocked);
    assert!(blocked_qa.request.is_none());
    task.validate().unwrap();
    let request = qa_request(&task);
    assert_ne!(request.code_head_sha, request.complete_diff_hash);
    task.apply(
        task.revision,
        TaskEvent::RuntimeQaRequested(request.clone()),
    )
    .unwrap();
    let mut stale_head = request.clone();
    stale_head.code_head_sha = request.complete_diff_hash.clone();
    assert!(
        task.apply(task.revision, TaskEvent::RuntimeQaRequested(stale_head))
            .is_err()
    );
    let mut stale_diff = request.clone();
    stale_diff.complete_diff_hash = "9".repeat(64);
    assert!(
        task.apply(task.revision, TaskEvent::RuntimeQaRequested(stale_diff))
            .is_err()
    );
    let qa_event = completed_qa_event(&task, &request);
    let mut diff_bound_report = qa_event.clone();
    if let TaskEvent::RuntimeQaCompleted { receipt, .. } = &mut diff_bound_report {
        receipt.subject_sha256 = request.code_head_sha.clone();
    }
    assert!(task.apply(task.revision, diff_bound_report).is_err());
    task.apply(task.revision, qa_event).unwrap();
    let proposal = crate::harness::prompt_catalog::SpecDeltaV1 {
        deltas: vec![crate::harness::prompt_catalog::SpecChangeV1 {
            requirement_id: "FR-1".into(),
            old: "- **FR-1** current behavior".into(),
            new: "- **FR-1** verified behavior".into(),
            classification: crate::harness::prompt_catalog::DeltaClassification::CurrentTruth,
            evidence: vec!["runtime-qa:run-1".into()],
            approval_effect: crate::harness::prompt_catalog::ApprovalEffect::RequiresUserApproval,
            followup_task_ids: Vec::new(),
        }],
    };
    let proposal_json = serde_json::to_string(&proposal).unwrap();
    let proposal_sha256 = format!("{:x}", sha2::Sha256::digest(proposal_json.as_bytes()));
    let blocker = TaskEvent::SpecReconciliationBlocked {
        reason: "The reconciler proposed a behavior change that needs review.".into(),
        proposal_sha256,
        spec_sha256: "8".repeat(64),
        proposal_json: Some(proposal_json.clone()),
    };
    task.apply(task.revision, blocker).unwrap();
    assert_eq!(task.phase, TaskPhase::AwaitingSpecReconciliation);
    assert!(task.spec_reconciliation_blocker.is_some());
    assert_eq!(
        task.spec_reconciliation_proposal_json.as_deref(),
        Some(proposal_json.as_str())
    );
    assert!(
        task.apply(
            task.revision,
            TaskEvent::SpecReconciliationRetry {
                current_spec_sha256: "8".repeat(64),
            },
        )
        .is_err()
    );
    task.apply(
        task.revision,
        TaskEvent::SpecReconciliationRetry {
            current_spec_sha256: "9".repeat(64),
        },
    )
    .unwrap();
    assert!(task.spec_reconciliation_blocker.is_none());
    assert!(task.spec_reconciliation_proposal_json.is_none());
    let spec = GateReceipt {
        receipt_uid: id(),
        subject_uid: task_uid,
        subject_sha256: code_sha,
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 2,
        evidence_refs: vec!["specification/revision-2".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    assert!(
        task.apply(
            task.revision,
            TaskEvent::ReconcileSpec {
                receipt: spec.clone(),
                pr_work_item_uid: id(),
            },
        )
        .is_err()
    );
    task.apply(
        task.revision,
        TaskEvent::SubtaskReconciled { receipt: spec },
    )
    .unwrap();
    assert_eq!(task.phase, TaskPhase::Done);
    assert_eq!(task.pr_work_item_uid, None);
    task.validate().unwrap();
    assert!(task.apply(task.revision, TaskEvent::Cancel).is_err());
}

#[test]
fn repair_cycles_and_review_history_are_bounded_and_replay_safe() {
    let task_uid = id();
    let mut task = TaskRecord {
        schema_version: 1,
        task_uid: task_uid.clone(),
        feature_uid: id(),
        planning_item_uid: None,
        task_spec_sha256: None,
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: Vec::new(),
        phase: TaskPhase::Ready,
        revision: 0,
        approved_plan_sha256: "b".repeat(64),
        implementation_sha256: None,
        reviewed_code_head_sha: None,
        reviewed_code_subject_sha256: None,
        code_review: None,
        review_history: Vec::new(),
        repair_attempts: 0,
        runtime_qa: None,
        qa_execution: None,
        spec_reconciliation: None,
        spec_reconciliation_blocker: None,
        spec_reconciliation_proposal_sha256: None,
        spec_reconciliation_spec_sha256: None,
        spec_reconciliation_proposal_json: None,
        pr_work_item_uid: None,
    };
    task.apply(0, TaskEvent::Start).unwrap();
    let mut code_sha = "c".repeat(64);
    task.apply(
        1,
        TaskEvent::ImplementationFinished {
            code_sha256: code_sha.clone(),
        },
    )
    .unwrap();
    let mut prior_receipt: Option<String> = None;
    for attempt in 0_u8..4 {
        let mut review = receipt(
            GateActor::IndependentReviewer,
            GateDecision::ChangesRequested,
            &task_uid,
        );
        review.subject_sha256 = code_sha.clone();
        if let Some(prior) = &prior_receipt {
            review.receipt_uid = prior.clone();
            assert!(
                task.apply(task.revision, code_review_event(review.clone()))
                    .is_err()
            );
            review.receipt_uid = id();
        }
        prior_receipt = Some(review.receipt_uid.clone());
        task.apply(task.revision, code_review_event(review))
            .unwrap();
        if attempt == 3 {
            assert!(task.apply(task.revision, TaskEvent::BeginRepair).is_err());
            break;
        }
        task.apply(task.revision, TaskEvent::BeginRepair).unwrap();
        code_sha = format!("{}{}", (b'd' + attempt) as char, "e".repeat(63));
        task.apply(
            task.revision,
            TaskEvent::ImplementationFinished {
                code_sha256: code_sha.clone(),
            },
        )
        .unwrap();
    }
    assert_eq!(task.repair_attempts, 3);
    assert!(task.repair_budget_exhausted());
    assert_eq!(task.review_history.len(), 4);
}
