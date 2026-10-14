use super::super::{feature, receipt, set_prompt_role};
use super::Fixture;
use crate::core::lifecycle::{
    FeatureEvent, FeaturePhase, GateActor, GateDecision, LifecycleStore, TaskPhase, TaskRecord,
};

fn approved_feature(lifecycle: &LifecycleStore<'_>) -> (String, String) {
    let mut feature = feature();
    let feature_uid = feature.feature_uid.clone();
    let snapshot_uid = uuid::Uuid::new_v4().hyphenated().to_string();
    let plan_sha = "a".repeat(64);
    feature.plan_snapshot_uid = Some(snapshot_uid.clone());
    feature.plan_snapshot_sha256 = Some(plan_sha.clone());
    lifecycle
        .create_feature(&feature, &uuid::Uuid::new_v4().to_string())
        .unwrap();
    let mut feature = lifecycle.load_feature(&feature_uid).unwrap();
    feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::ReadinessRecorded {
                evidence_refs: vec!["readiness/current-spec".into()],
            },
        )
        .unwrap();
    feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::PlanReady {
                plan_sha256: plan_sha.clone(),
            },
        )
        .unwrap();
    let mut user = receipt(GateActor::User, GateDecision::Approved, &snapshot_uid);
    user.subject_sha256 = plan_sha.clone();
    feature = lifecycle
        .apply_feature_event(
            &feature_uid,
            feature.revision,
            &uuid::Uuid::new_v4().to_string(),
            FeatureEvent::UserPlanDecision(user),
        )
        .unwrap();
    let mut review = receipt(
        GateActor::IndependentReviewer,
        GateDecision::Approved,
        &snapshot_uid,
    );
    set_prompt_role(
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
    assert_eq!(feature.phase, FeaturePhase::ReadyToDecompose);
    (feature_uid, plan_sha)
}

fn task(feature_uid: &str, plan_sha: &str, planning_uid: &str) -> TaskRecord {
    let namespace = uuid::Uuid::parse_str(feature_uid).unwrap();
    let spec_sha = format!("{}{}", plan_sha, planning_uid);
    let task_uid = uuid::Uuid::new_v5(&namespace, spec_sha.as_bytes())
        .hyphenated()
        .to_string();
    TaskRecord {
        schema_version: 1,
        task_uid,
        feature_uid: feature_uid.into(),
        planning_item_uid: Some(planning_uid.into()),
        task_spec_sha256: Some("d".repeat(64)),
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        phase: TaskPhase::Ready,
        revision: 0,
        approved_plan_sha256: plan_sha.into(),
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
    }
}

#[test]
fn task_batch_retries_resume_partial_writes_and_reject_changed_bindings() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let lifecycle = LifecycleStore::new(&store);
    let (feature_uid, plan_sha) = approved_feature(&lifecycle);
    let first = task(
        &feature_uid,
        &plan_sha,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let mut second = task(
        &feature_uid,
        &plan_sha,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    second.parent_task_uid = Some(first.task_uid.clone());
    let mut third = task(
        &feature_uid,
        &plan_sha,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    third.parent_task_uid = Some(first.task_uid.clone());
    third.dependency_task_uids = vec![second.task_uid.clone()];
    let mut first = first;
    first.subtask_uids = vec![second.task_uid.clone(), third.task_uid.clone()];

    lifecycle
        .reconcile_task_batch(&feature_uid, &plan_sha, std::slice::from_ref(&first))
        .unwrap();
    let reopened_store = fixture.store();
    let reopened = LifecycleStore::new(&reopened_store);
    reopened
        .reconcile_task_batch(
            &feature_uid,
            &plan_sha,
            &[first.clone(), second.clone(), third.clone()],
        )
        .unwrap();
    reopened
        .reconcile_task_batch(
            &feature_uid,
            &plan_sha,
            &[first.clone(), second.clone(), third.clone()],
        )
        .unwrap();

    let feature = reopened.load_feature(&feature_uid).unwrap();
    assert_eq!(feature.phase, FeaturePhase::Executing);
    assert_eq!(feature.task_uids.len(), 3);
    assert_eq!(reopened.load_task(&first.task_uid).unwrap().revision, 1);
    assert_eq!(
        reopened
            .task_for_planning_item(first.planning_item_uid.as_deref().unwrap())
            .unwrap()
            .unwrap()
            .task_uid,
        first.task_uid
    );
    assert_eq!(reopened.load_task(&second.task_uid).unwrap().revision, 1);
    assert_eq!(reopened.load_task(&third.task_uid).unwrap().revision, 1);
    assert_eq!(
        reopened.load_task(&first.task_uid).unwrap().subtask_uids,
        [second.task_uid.clone(), third.task_uid.clone()]
    );
    assert_eq!(
        reopened
            .load_task(&third.task_uid)
            .unwrap()
            .dependency_task_uids,
        [second.task_uid.clone()]
    );

    let mut changed = first;
    changed.task_spec_sha256 = Some("e".repeat(64));
    assert!(
        reopened
            .reconcile_task_batch(&feature_uid, &plan_sha, &[changed, second, third])
            .is_err()
    );
}
