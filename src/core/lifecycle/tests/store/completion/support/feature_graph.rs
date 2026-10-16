use crate::core::lifecycle::{
    FeatureEvent, GateActor, GateDecision, GateReceipt, LifecycleStore, TaskEvent, TaskPhase,
    TaskRecord,
};

fn complete_subtask(lifecycle: &LifecycleStore<'_>, task_uid: &str) {
    let mut task = lifecycle.load_task(task_uid).unwrap();
    task = lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::Start,
        )
        .unwrap();
    let code_sha = "d".repeat(64);
    task = lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::ImplementationFinished {
                code_sha256: code_sha.clone(),
            },
        )
        .unwrap();
    let mut review = super::super::super::super::receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        task_uid,
    );
    super::super::super::super::set_prompt_role(
        &mut review,
        crate::harness::prompt_catalog::PromptRole::CodeReviewer,
    );
    review.subject_sha256 = code_sha.clone();
    task = lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            super::super::super::super::code_review_event(review),
        )
        .unwrap();
    let request = super::super::super::super::qa_request(&task);
    task = lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::RuntimeQaRequested(request.clone()),
        )
        .unwrap();
    let qa_event = super::super::super::super::completed_qa_event(&task, &request);
    task = lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            qa_event,
        )
        .unwrap();
    let spec = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: task_uid.into(),
        subject_sha256: code_sha,
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
    };
    lifecycle
        .apply_task_event(
            task_uid,
            task.revision,
            &uuid::Uuid::new_v4().to_string(),
            TaskEvent::SubtaskReconciled { receipt: spec },
        )
        .unwrap();
}

pub(in crate::core::lifecycle::tests::store::completion) fn create_feature_graph(
    lifecycle: &LifecycleStore<'_>,
) -> (String, String, String, u64) {
    let mut feature = super::super::super::super::feature();
    let feature_uid = feature.feature_uid.clone();
    lifecycle
        .create_feature(&feature, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    feature = lifecycle.load_feature(&feature_uid).unwrap();
    lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::ReadinessRecorded {
                evidence_refs: vec!["spec/acceptance-criteria".into()],
            },
        )
        .unwrap();
    let feature = lifecycle.load_feature(&feature_uid).unwrap();
    let plan_sha = feature.plan_sha256();
    let feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::PlanReady {
                plan_sha256: plan_sha.clone(),
            },
        )
        .unwrap();
    let mut user_receipt =
        super::super::super::super::receipt(GateActor::User, GateDecision::Approved, &feature_uid);
    user_receipt.subject_sha256 = plan_sha.clone();
    let feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::UserPlanDecision(user_receipt),
        )
        .unwrap();
    let mut review = super::super::super::super::receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &feature_uid,
    );
    super::super::super::super::set_prompt_role(
        &mut review,
        crate::harness::prompt_catalog::PromptRole::PlanReviewer,
    );
    review.subject_sha256 = plan_sha.clone();
    let feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::PlanReview(review),
        )
        .unwrap();
    lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::TaskGraphCreated {
                plan_sha256: plan_sha.clone(),
            },
        )
        .unwrap();

    let feature_revision_before_task_creation =
        lifecycle.load_feature(&feature_uid).unwrap().revision;
    let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    lifecycle
        .create_task(
            &TaskRecord {
                schema_version: 1,
                task_uid: task_uid.clone(),
                feature_uid: feature_uid.clone(),
                planning_item_uid: None,
                task_spec_sha256: None,
                parent_task_uid: None,
                subtask_uids: Vec::new(),
                dependency_task_uids: Vec::new(),
                approved_requirement_ids: Vec::new(),
                phase: TaskPhase::Ready,
                revision: 0,
                approved_plan_sha256: plan_sha,
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
            },
            &uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
    let child_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    lifecycle
        .create_task(
            &TaskRecord {
                schema_version: 1,
                task_uid: child_uid.clone(),
                feature_uid: feature_uid.clone(),
                planning_item_uid: None,
                task_spec_sha256: None,
                parent_task_uid: Some(task_uid.clone()),
                subtask_uids: Vec::new(),
                dependency_task_uids: Vec::new(),
                approved_requirement_ids: Vec::new(),
                phase: TaskPhase::Ready,
                revision: 0,
                approved_plan_sha256: feature.approved_plan_sha256.clone().unwrap(),
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
            },
            &uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
    assert_eq!(
        lifecycle
            .load_task(&task_uid)
            .unwrap()
            .subtask_uids
            .as_slice(),
        std::slice::from_ref(&child_uid)
    );
    assert!(
        lifecycle
            .events_for_feature(&feature_uid)
            .unwrap()
            .iter()
            .any(|event| {
                event.task_uid.as_deref() == Some(child_uid.as_str())
                    && matches!(
                        &event.event,
                        crate::core::lifecycle::LifecycleEventKind::SubtaskCreated {
                            parent_task_uid
                        } if parent_task_uid == &task_uid
                    )
            })
    );
    complete_subtask(lifecycle, &child_uid);
    let feature_after_task_creation = lifecycle.load_feature(&feature_uid).unwrap();
    assert_eq!(
        feature_after_task_creation.task_uids,
        [task_uid.clone(), child_uid.clone()]
    );
    assert!(matches!(
        lifecycle.apply_feature_event(
            &feature_uid,
            feature_revision_before_task_creation,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::TasksCompleted,
        ),
        Err(crate::artifacts::planning_store::StoreError::StaleRecordRevision { .. })
    ));
    assert!(matches!(
        lifecycle.apply_feature_event(
            &feature_uid,
            feature_after_task_creation.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::TasksCompleted,
        ),
        Err(crate::artifacts::planning_store::StoreError::MalformedState(_))
    ));
    (
        feature_uid,
        task_uid,
        child_uid,
        feature_revision_before_task_creation,
    )
}
