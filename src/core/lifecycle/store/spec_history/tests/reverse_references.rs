use super::*;
use crate::core::lifecycle::{
    GateActor, GateDecision, GateReceipt, LifecycleStore, PullRequestPreparation,
    PullRequestPreparationStatus, SpecDeltaClassificationV1, SpecDeltaRecordV1,
    SpecReconciliationStatusV1,
};
#[path = "reverse_references/assertions.rs"]
mod assertions;
#[path = "reverse_references/support.rs"]
mod support;
use assertions::assert_reverse_references;
use support::{Persisted, code_review_receipt};

#[test]
// Production boundary: persisted lifecycle history, task/receipt and PR-state
// records, and the same reverse lookup used by UI links. A trace-only unit test
// would miss typed reference admission. Uses local temp files and synthetic
// records; no provider or external service is involved.
fn reverse_lookup_resolves_decision_task_review_qa_and_pr_state_references() {
    use crate::core::lifecycle::{QaExecutionStateV1, QaExecutionStatusV1, TaskPhase, TaskRecord};
    let fixture = Fixture::new();
    let store = fixture.store();
    let (feature_uid, _) = traceability::support::reviewed_feature(
        &fixture,
        &store,
        "CHG-111",
        "CHG-111-reverse-refs",
    );
    let lifecycle = LifecycleStore::new(&store);
    let feature = lifecycle.load_feature(&feature_uid).unwrap();
    let snapshot = crate::core::lifecycle::plan_snapshot::load_persisted(
        &store,
        feature.plan_snapshot_uid.as_deref().unwrap(),
    )
    .unwrap()
    .unwrap();
    let requirement_id = snapshot.requirements[0].stable_id.clone();
    let decision_ref = traceability::support::answered_decision(
        &store,
        &feature_uid,
        &requirement_id,
        "reverse trace decision",
    );
    let question_store = crate::core::planning::QuestionStore::new(&store);
    let (mut readiness, readiness_revision) = question_store
        .load_readiness(&feature_uid)
        .unwrap()
        .unwrap();
    readiness.acceptance_tests[0].test_ref = "tests/reverse_reference.rs".into();
    readiness.revision += 1;
    question_store
        .save_readiness(&readiness, readiness_revision)
        .unwrap();
    let plan_hash = feature.approved_plan_sha256.clone().unwrap();
    lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().hyphenated().to_string(),
            crate::core::lifecycle::FeatureEvent::TaskGraphCreated {
                plan_sha256: plan_hash.clone(),
            },
        )
        .unwrap();

    let task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let work_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let implementation_hash = "d".repeat(64);
    let reviewed_head = "e".repeat(40);
    let review_subject = "f".repeat(64);
    let code_review = code_review_receipt(
        &store,
        &feature_uid,
        snapshot.uid.as_str(),
        &task_uid,
        &plan_hash,
        &implementation_hash,
    );
    let qa_request = crate::runtime_qa::QaRunRequestV1 {
        schema_version: 1,
        qa_run_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        parent_task_uid: task_uid.clone(),
        planning_item_uid: None,
        feature_uid: feature_uid.clone(),
        code_head_sha: reviewed_head.clone(),
        build_artifact_sha256: "a".repeat(64),
        complete_diff_hash: implementation_hash.clone(),
        review_subject_sha256: review_subject.clone(),
        approved_spec_hash: plan_hash.clone(),
        requirement_ids: vec![requirement_id.clone()],
        acceptance_criteria: vec!["The stored trace resolves to this requirement.".into()],
        relevant_platforms: vec![crate::runtime_qa::QaPlatform::Web],
        startup_recipe_ref: "recipe:synthetic/v1".into(),
        startup_recipe_sha256: "b".repeat(64),
        execution_session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        driver_profile_version: "fixture-driver/v1".into(),
        test_data_fixture_ref: None,
        dependency_cache_refs: Vec::new(),
        bounded_limits: crate::runtime_qa::QaBoundedLimitsV1 {
            max_scenarios: 1,
            max_actions_per_scenario: 1,
            max_total_actions: 1,
            timeout_seconds: 30,
            max_evidence_bytes: 1024,
            max_screenshots: 1,
        },
        author_run_id: code_review.author_run_id.clone().unwrap(),
        author_session_id: code_review.author_session_id.clone().unwrap(),
    };
    let qa_receipt = GateReceipt {
        receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
        subject_uid: task_uid.clone(),
        subject_sha256: implementation_hash.clone(),
        actor: GateActor::ApplicationPolicy,
        decision: GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec!["fixture:runtime-qa-result".into()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    let task = TaskRecord {
        schema_version: 1,
        task_uid: task_uid.clone(),
        feature_uid: feature_uid.clone(),
        planning_item_uid: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
        task_spec_sha256: Some("c".repeat(64)),
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: vec![requirement_id.clone()],
        phase: TaskPhase::Ready,
        revision: 0,
        approved_plan_sha256: plan_hash.clone(),
        implementation_sha256: Some(implementation_hash.clone()),
        reviewed_code_head_sha: Some(reviewed_head),
        reviewed_code_subject_sha256: Some(review_subject),
        code_review: Some(code_review.clone()),
        review_history: Vec::new(),
        repair_attempts: 0,
        runtime_qa: Some(qa_receipt.clone()),
        qa_execution: Some(QaExecutionStateV1 {
            schema_version: 1,
            status: QaExecutionStatusV1::Completed,
            request: Some(qa_request),
            blocked_reason: None,
            result_sha256: Some("1".repeat(64)),
            evidence_manifest_sha256: Some("2".repeat(64)),
            reviewer_execution_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
            reviewer_session_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
            applicability: None,
        }),
        spec_reconciliation: None,
        spec_reconciliation_blocker: None,
        spec_reconciliation_proposal_sha256: None,
        spec_reconciliation_spec_sha256: None,
        spec_reconciliation_proposal_json: None,
        pr_work_item_uid: Some(work_uid.clone()),
    };
    let pr = PullRequestPreparation {
        schema_version: 1,
        uid: work_uid.clone(),
        work_item_uid: work_uid.clone(),
        feature_uid: feature_uid.clone(),
        task_uid: task_uid.clone(),
        included_task_uids: vec![task_uid.clone()],
        revision: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
        status: PullRequestPreparationStatus::Merged,
        publication_authorization: Some(GateReceipt {
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            subject_uid: work_uid.clone(),
            subject_sha256: "a".repeat(64),
            actor: GateActor::User,
            decision: GateDecision::Approved,
            recorded_at_unix_ms: 1,
            evidence_refs: vec![format!("user-action:create-pr:{work_uid}")],
            prompt_sha256: None,
            prompt_manifest: None,
            reviewer_run_id: None,
            author_run_id: None,
            reviewer_session_id: None,
            author_session_id: None,
        }),
        provider_pr_id: Some(11),
        provider_pr_url: Some("https://github.com/example/trace-fixture/pull/11".into()),
        head_sha: Some("3".repeat(40)),
        base_sha: Some("4".repeat(40)),
        merged_commit_sha: Some("5".repeat(40)),
    };
    let pr_path = format!("state/lifecycle_pr_work_items/{work_uid}.json");
    store
        .save_record(
            &pr_path,
            &Persisted {
                uid: work_uid.clone(),
                schema_version: 1,
                created_at_ms: 1,
                updated_at_ms: 1,
                revision: 1,
                record: pr,
            },
            0,
        )
        .unwrap();
    lifecycle
        .create_task(&task, &uuid::Uuid::new_v4().hyphenated().to_string())
        .unwrap();
    let task_event = uuid::Uuid::new_v4().hyphenated().to_string();
    let mut followup_task = task.clone();
    followup_task.task_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    followup_task.planning_item_uid = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    followup_task.parent_task_uid = None;
    followup_task.subtask_uids.clear();
    followup_task.dependency_task_uids.clear();
    followup_task.pr_work_item_uid = None;
    followup_task.code_review = None;
    followup_task.reviewed_code_head_sha = None;
    followup_task.reviewed_code_subject_sha256 = None;
    followup_task.runtime_qa = None;
    followup_task.qa_execution = None;
    lifecycle.create_task(&followup_task, &task_event).unwrap();
    let mut changed_retry = followup_task.clone();
    changed_retry.approved_requirement_ids.clear();
    assert!(lifecycle.create_task(&changed_retry, &task_event).is_err());

    let feature = lifecycle.load_feature(&feature_uid).unwrap();
    let trigger = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "reverse-reference-edges")
        .unwrap();
    let pr_ref = format!("provider-pr-state:{work_uid}:1");
    let mut draft = Fixture::draft(&store, feature.revision, trigger.uid.clone());
    draft.decision_refs = feature.decision_refs.clone();
    let operator_decision_ref = uuid::Uuid::new_v4().hyphenated().to_string();
    draft.decision_refs.push(operator_decision_ref.clone());
    draft.approved_target_hash = feature.approved_plan_sha256.clone();
    draft.reconciliation_status = SpecReconciliationStatusV1::OutstandingNonblockingFindings;
    let authorization = assertions::declined_authorization(
        &operator_decision_ref,
        &feature_uid,
        &requirement_id,
        &trigger.uid,
        "Old trace wording",
        "New trace wording",
    );
    draft.deltas.push(SpecDeltaRecordV1 {
        requirement_id: requirement_id.clone(),
        event_id: trigger.uid.clone(),
        old_target: Some("Old trace wording".into()),
        proposed_target: Some("New trace wording".into()),
        observed_current: None,
        classification: SpecDeltaClassificationV1::Documentation,
        evidence_refs: vec![format!("spec-trigger:{}", trigger.uid), pr_ref.clone()],
        decision_refs: vec![
            decision_ref.clone(),
            code_review.receipt_uid.clone(),
            qa_receipt.receipt_uid.clone(),
        ],
        test_refs: vec!["tests/reverse_reference.rs".into()],
        affected_task_ids: vec![task_uid.clone()],
        needs_user_reapproval: false,
        followup_ids: vec![followup_task.task_uid.clone()],
        operator_decision: Some(crate::core::lifecycle::SpecDeltaOperatorDecisionV1 {
            receipt_uid: operator_decision_ref.clone(),
            decision: crate::core::lifecycle::SpecDeltaOperatorChoiceV1::Declined,
            actor: GateActor::User,
            recorded_at_ms: 1,
            resulting_spec_sha256: None,
        }),
    });
    let store_revision = store.revision().unwrap();
    lifecycle
        .append_spec_revision_with_feature_authorization(
            &feature_uid,
            1,
            draft,
            Some(&store_revision),
            authorization,
            Vec::new(),
        )
        .unwrap();

    let references = vec![
        decision_ref,
        task_uid,
        code_review.receipt_uid,
        qa_receipt.receipt_uid,
        pr_ref,
        followup_task.task_uid,
        operator_decision_ref,
    ];
    assert_reverse_references(&lifecycle, &feature_uid, &requirement_id, &references);
}
