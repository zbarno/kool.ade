use super::*;

#[test]
fn headless_not_applicable_requires_exact_reviewer_assessment_and_manager_signoff() {
    let task_uid = id();
    let mut task = TaskRecord {
        schema_version: 1,
        task_uid: task_uid.clone(),
        feature_uid: id(),
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
    task.apply(0, TaskEvent::Start).unwrap();
    task.apply(
        1,
        TaskEvent::ImplementationFinished {
            code_sha256: "c".repeat(64),
        },
    )
    .unwrap();
    let mut review = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &task_uid,
    );
    review.subject_sha256 = "c".repeat(64);
    task.apply(2, code_review_event(review)).unwrap();

    let assessment = crate::runtime_qa::QaApplicabilityAssessmentV1 {
        schema_version: 1,
        task_uid: task_uid.clone(),
        target_id: "service-api".into(),
        app_surface: crate::project_runtime::AppSurface::HeadlessService,
        code_sha: task.reviewed_code_head_sha.clone().unwrap(),
        approved_spec_hash: task.approved_plan_sha256.clone(),
        applicability: crate::runtime_qa::QaApplicability::NotApplicable,
        reason: "Approved target is a headless service; visual interaction does not apply".into(),
        evidence_refs: vec![
            "target:service-api".into(),
            "capability-manifest:headless-target".into(),
        ],
        reviewer_execution_id: id(),
        reviewer_session_id: id(),
        approved_by: id(),
        manager_session_id: id(),
        recorded_at_unix_ms: 1,
    };
    let assessment_hash = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&assessment).unwrap())
    );
    let mut reviewer_receipt = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &task_uid,
    );
    set_prompt_role(
        &mut reviewer_receipt,
        crate::harness::prompt_catalog::PromptRole::RuntimeQaReviewerV2,
    );
    reviewer_receipt.subject_sha256 = assessment_hash.clone();
    reviewer_receipt.reviewer_run_id = Some(assessment.reviewer_execution_id.clone());
    reviewer_receipt.reviewer_session_id = Some(assessment.reviewer_session_id.clone());
    reviewer_receipt.author_run_id = Some(assessment.approved_by.clone());
    reviewer_receipt.author_session_id = Some(assessment.manager_session_id.clone());
    reviewer_receipt.evidence_refs = vec![format!(
        "runtime-qa-applicability-assessment:{assessment_hash}"
    )];
    let mut signoff = receipt(
        GateActor::ApplicationPolicy,
        GateDecision::NotApplicable,
        &task_uid,
    );
    signoff.subject_sha256 = "c".repeat(64);
    signoff.reviewer_run_id = Some(assessment.approved_by.clone());
    signoff.reviewer_session_id = Some(assessment.manager_session_id.clone());
    signoff.evidence_refs = vec![
        format!("runtime-qa-applicability:{assessment_hash}"),
        format!("gate-receipt:{}", reviewer_receipt.receipt_uid),
    ];

    let root = tempfile::tempdir().unwrap();
    let planning = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.path(),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let lifecycle = LifecycleStore::new(&planning);
    assert!(matches!(
        lifecycle.apply_task_event(
            &task_uid,
            0,
            &id(),
            TaskEvent::RuntimeQaNotApplicable {
                assessment: Box::new(assessment.clone()),
                reviewer_receipt: Box::new(reviewer_receipt.clone()),
                receipt: Box::new(signoff.clone()),
            },
        ),
        Err(crate::artifacts::planning_store::StoreError::MalformedState(reason))
            if reason.contains("validated evidence paths")
    ));

    let mut previously_unavailable = task.clone();
    previously_unavailable
        .apply(
            previously_unavailable.revision,
            TaskEvent::RuntimeQaUnavailable {
                reason: "Runtime QA infrastructure is unavailable".into(),
            },
        )
        .unwrap();
    previously_unavailable
        .apply(
            previously_unavailable.revision,
            TaskEvent::RuntimeQaNotApplicable {
                assessment: Box::new(assessment.clone()),
                reviewer_receipt: Box::new(reviewer_receipt.clone()),
                receipt: Box::new(signoff.clone()),
            },
        )
        .unwrap();
    assert_eq!(
        previously_unavailable.qa_execution.as_ref().unwrap().status,
        QaExecutionStatusV1::NotApplicable
    );

    let mut blocked_request = task.clone();
    let request = qa_request(&blocked_request);
    blocked_request
        .apply(
            blocked_request.revision,
            TaskEvent::RuntimeQaRequested(request.clone()),
        )
        .unwrap();
    blocked_request
        .apply(
            blocked_request.revision,
            TaskEvent::RuntimeQaBlocked {
                request_id: request.qa_run_id,
                reason: "The requested QA run was blocked".into(),
                result_sha256: None,
                evidence_manifest_sha256: None,
            },
        )
        .unwrap();
    assert!(
        blocked_request
            .apply(
                blocked_request.revision,
                TaskEvent::RuntimeQaNotApplicable {
                    assessment: Box::new(assessment.clone()),
                    reviewer_receipt: Box::new(reviewer_receipt.clone()),
                    receipt: Box::new(signoff.clone()),
                },
            )
            .is_err()
    );

    let mut stale = assessment.clone();
    stale.code_sha = "2".repeat(40);
    assert!(
        task.clone()
            .apply(
                task.revision,
                TaskEvent::RuntimeQaNotApplicable {
                    assessment: Box::new(stale),
                    reviewer_receipt: Box::new(reviewer_receipt.clone()),
                    receipt: Box::new(signoff.clone()),
                },
            )
            .is_err()
    );

    task.apply(
        task.revision,
        TaskEvent::RuntimeQaNotApplicable {
            assessment: Box::new(assessment),
            reviewer_receipt: Box::new(reviewer_receipt),
            receipt: Box::new(signoff),
        },
    )
    .unwrap();
    assert_eq!(task.phase, TaskPhase::AwaitingSpecReconciliation);
    assert_eq!(
        task.qa_execution.as_ref().unwrap().status,
        QaExecutionStatusV1::NotApplicable
    );
    task.validate().unwrap();
}
