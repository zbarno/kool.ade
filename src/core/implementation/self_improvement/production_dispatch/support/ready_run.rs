use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::{
        lifecycle::{FeaturePhase, LifecycleStore, TaskPhase, TaskRecord},
        opportunity::{OpportunitySaveResult, OpportunityStore},
        self_improvement::{
            SelfImprovementEventV1, SelfImprovementPhaseV1, SelfImprovementRunV1,
            SelfImprovementStore,
        },
    },
    harness::prompt_catalog::{OpportunityChoice, OpportunityDecisionV1},
};
use std::path::Path;

pub(crate) struct ReadyRunFixture {
    pub(crate) planning: PlanningStore,
    pub(crate) workspace_uid: String,
    pub(crate) lifecycle_task_uid: String,
    pub(crate) planning_item_uid: String,
    pub(crate) ready: SelfImprovementRunV1,
}

pub(crate) fn build_ready_run(temp: &Path, repo: &Path, source_sha: &str) -> ReadyRunFixture {
    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.join("planning"),
        StoreMode::ManagedLocal,
    );
    crate::artifacts::migration::bootstrap_product_with_store(repo, &planning, "Auth fixture")
        .unwrap();
    let workspace_uid = planning.project_id.hyphenated().to_string();
    let repository_uid =
        crate::core::project_repos::stable_uid(&workspace_uid, "repository", "root");
    let manifest = crate::core::project_repos::ProjectManifest {
        repositories: vec![crate::core::project_repos::Repository {
            id: "root".into(),
            role: "Product source".into(),
            remote: "https://github.com/zbarno/kool.ade".into(),
            display_name: None,
        }],
    };
    planning
        .transaction(
            &[(
                crate::artifacts::planning_store::paths::PROJECT_MANIFEST.into(),
                serde_json::to_vec(&manifest).unwrap(),
            )],
            None,
        )
        .unwrap();

    let (snapshot, user_receipt, review_receipt) = super::super::approved_plan(&planning);
    let feature_uid = snapshot.feature_uid.clone();
    let lifecycle = LifecycleStore::new(&planning);
    let feature = lifecycle
        .reconcile_reviewed_plan(
            &snapshot,
            "<!-- opportunity-uid: fixture -->\n# Approved self-improvement\n",
            vec!["test-fixture:readiness".into()],
            &user_receipt,
            &review_receipt,
        )
        .unwrap();
    assert_eq!(feature.phase, FeaturePhase::ReadyToDecompose);
    let lifecycle_task_uid = uuid::Uuid::new_v4().to_string();
    let planning_item_uid = uuid::Uuid::new_v4().to_string();
    let task = TaskRecord {
        schema_version: 1,
        task_uid: lifecycle_task_uid.clone(),
        feature_uid: feature_uid.clone(),
        planning_item_uid: Some(planning_item_uid.clone()),
        task_spec_sha256: Some("e".repeat(64)),
        parent_task_uid: None,
        subtask_uids: Vec::new(),
        dependency_task_uids: Vec::new(),
        approved_requirement_ids: vec![snapshot.requirements[0].stable_id.clone()],
        phase: TaskPhase::Ready,
        revision: 0,
        approved_plan_sha256: snapshot.content_hash.clone(),
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
    lifecycle
        .reconcile_task_batch(&feature_uid, &snapshot.content_hash, &[task])
        .unwrap();

    let mut proposal = super::super::opportunity(&workspace_uid, &repository_uid, source_sha);
    let opportunities = OpportunityStore::new(&planning);
    let OpportunitySaveResult::Created {
        proposal: stored_proposal,
        record_revision,
    } = opportunities.save_advisory(proposal.clone()).unwrap()
    else {
        panic!("fixture opportunity should be new");
    };
    proposal = stored_proposal;
    let decision = OpportunityDecisionV1 {
        uid: uuid::Uuid::new_v4().to_string(),
        opportunity_uid: proposal.opportunity_uid.clone(),
        revision_hash: proposal.revision_hash().unwrap(),
        user_id: "fixture-operator".into(),
        decision: OpportunityChoice::Pursue,
        occurred_at: "2026-10-19T12:00:00Z".into(),
        snooze_until: None,
        explanation: Some("Explicit fixture choice".into()),
    };
    opportunities
        .record_user_decision(record_revision, &decision, "fixture-operator")
        .unwrap();
    let runs = SelfImprovementStore::new(&planning);
    let run = runs
        .create_from_pursued(
            &opportunities,
            &decision.uid,
            super::super::self_improvement_run(&decision, &proposal, &feature_uid),
        )
        .unwrap();
    let ready = runs
        .apply_event(
            &run.run_uid,
            run.revision,
            &uuid::Uuid::new_v4().to_string(),
            SelfImprovementEventV1::PlanApproved {
                approval_sha256: snapshot.content_hash.clone(),
                capability_id: Some("self-improvement-lifecycle".into()),
                review_uid: review_receipt.receipt_uid.clone(),
                source_ref: "koolade/self-improvement-fixture".into(),
                destination_ref: "main".into(),
                task_uids: vec![lifecycle_task_uid.clone()],
                owner_task_uid: lifecycle_task_uid.clone(),
                parent_task_uids: Vec::new(),
            },
        )
        .unwrap();
    assert_eq!(ready.phase, SelfImprovementPhaseV1::Ready);
    let mut mismatched_capability = ready.clone();
    mismatched_capability.capability_id = Some("different-capability".into());
    assert!(
        super::super::verify_plan_binding(
            &planning,
            &mismatched_capability,
            &feature_uid,
            &lifecycle_task_uid,
        )
        .is_err()
    );

    ReadyRunFixture {
        planning,
        workspace_uid,
        lifecycle_task_uid,
        planning_item_uid,
        ready,
    }
}
