use super::LifecycleFixture;
use crate::{
    artifacts::planning_store::paths,
    core::{
        opportunity::{
            EvidenceConfidentialityV1, OpportunityDecisionKindV1, OpportunityEvidenceRefV1,
            OpportunityExploreModeV1, OpportunityKindV1, OpportunityOriginV1,
            OpportunitySaveResult, OpportunityScopeV1, OpportunityStatusV1, OpportunityStore,
            OpportunityTargetKindV1, OpportunityTargetV1, OpportunityV1,
        },
        project_repos::{ProjectManifest, stable_uid},
        self_improvement::{
            SelfImprovementEventV1 as Event, SelfImprovementPhaseV1 as Phase, SelfImprovementRunV1,
            SelfImprovementStore,
        },
    },
    harness::prompt_catalog::{OpportunityChoice, OpportunityDecisionV1 as UserDecision},
};

pub(super) fn seed_pr_pending_run(fixture: &LifecycleFixture, source_sha: &str) -> String {
    let store = &fixture.store;
    let workspace_uid = store.project_id.hyphenated().to_string();
    let mut manifest = ProjectManifest::load(store).unwrap();
    manifest
        .repositories
        .iter_mut()
        .find(|repository| repository.id == "root")
        .expect("product fixture has a root repository")
        .remote = "https://github.com/zbarno/kool.ade".into();
    store
        .transaction(
            &[(
                paths::PROJECT_MANIFEST.to_owned(),
                serde_json::to_vec(&manifest).unwrap(),
            )],
            None,
        )
        .unwrap();
    let repository_uid = stable_uid(&workspace_uid, "repository", "root");
    let opportunities = OpportunityStore::new(store);
    let settings = opportunities.discovery_settings().unwrap();
    opportunities
        .save_discovery_settings(
            &crate::core::opportunity::OpportunityDiscoverySettingsV1 {
                exploration: OpportunityExploreModeV1::OptedIn,
                ..settings.clone()
            },
            settings.revision,
        )
        .unwrap();
    let proposal = OpportunityV1 {
        schema_version: 1,
        opportunity_uid: uid(),
        workspace_uid,
        origin: OpportunityOriginV1::ScheduledExploration,
        kind: OpportunityKindV1::Quality,
        target: OpportunityTargetV1 {
            kind: OpportunityTargetKindV1::KooladeProduct,
            repository_uid: repository_uid.clone(),
            canonical_remote: "https://github.com/zbarno/kool.ade".into(),
            source_sha: source_sha.into(),
        },
        related_feature_uids: Vec::new(),
        related_task_uids: Vec::new(),
        discovered_at_ms: 1,
        observation_revision: 1,
        evidence_refs: vec![OpportunityEvidenceRefV1 {
            evidence_uid: uid(),
            sha256: "b".repeat(64),
            confidentiality: EvidenceConfidentialityV1::Public,
        }],
        problem_or_opportunity: "Verify app projection after provider recovery".into(),
        user_value: "Show publication progress after restart".into(),
        expected_outcome: "The persisted Evolve run opens the recovered PR once".into(),
        possible_approaches: vec!["Reuse app-owned publication state".into()],
        recommended_approach: "Project the persisted lifecycle result".into(),
        alternatives: vec!["Manually repair the run".into()],
        estimated_scope: OpportunityScopeV1::Small,
        impact: "Keeps the board aligned with publication".into(),
        cost_or_risk: "Local integration test setup".into(),
        affected_systems: vec!["publication".into(), "self-improvement".into()],
        current_capability_state: "The app projection is not exercised here".into(),
        generic_workaround: None,
        proposed_approach: "Cover the combined production boundary".into(),
        dedup_fingerprint: "c".repeat(64),
        status: OpportunityStatusV1::Proposed,
        revision: 1,
    };
    let OpportunitySaveResult::Created {
        proposal,
        record_revision,
    } = opportunities.save_advisory(proposal).unwrap()
    else {
        panic!("fixture opportunity should be new")
    };
    let decision = UserDecision {
        uid: uid(),
        opportunity_uid: proposal.opportunity_uid.clone(),
        revision_hash: proposal.revision_hash().unwrap(),
        user_id: "synthetic-controller".into(),
        decision: OpportunityChoice::Pursue,
        occurred_at: "2026-10-20T12:00:00Z".into(),
        snooze_until: None,
        explanation: Some("Synthetic explicit Pursue action".into()),
    };
    let decision = opportunities
        .record_user_decision(record_revision, &decision, "synthetic-controller")
        .unwrap();
    let crate::core::opportunity::OpportunityDecisionResult::Recorded { decision, .. } = decision
    else {
        panic!("fixture Pursue action should be new")
    };
    assert_eq!(decision.decision, OpportunityDecisionKindV1::Pursue);

    let run_uid = SelfImprovementStore::run_uid_for_decision(&decision.uid).unwrap();
    let mut run = SelfImprovementStore::new(store)
        .create_from_pursued(
            &opportunities,
            &decision.uid,
            SelfImprovementRunV1 {
                schema_version: 3,
                run_uid: run_uid.clone(),
                opportunity_uid: proposal.opportunity_uid,
                approved_decision_uid: decision.uid.clone(),
                target_repository_uid: repository_uid,
                canonical_remote: "https://github.com/zbarno/kool.ade".into(),
                source_ref: None,
                source_sha: source_sha.into(),
                destination_ref: None,
                feature_uid: fixture.task.feature_uid.clone(),
                parent_run_uid: None,
                chain_depth: 0,
                capability_id: None,
                task_uids: Vec::new(),
                owner_task_uid: None,
                parent_task_progress: Vec::new(),
                clone_uid: None,
                plan_approval_hash: None,
                independent_plan_review_uid: None,
                implementation_head_sha: None,
                implementation_subject_sha256: None,
                code_review_uids: Vec::new(),
                runtime_qa_uids: Vec::new(),
                spec_receipt_uid: None,
                pr_work_item_uid: None,
                pr_url: None,
                merged_sha: None,
                installed_version: None,
                installed_build_commit_sha: None,
                activated_version: None,
                phase: Phase::ApprovedToPlan,
                revision: 0,
            },
        )
        .unwrap();
    let runs = SelfImprovementStore::new(store);
    apply(
        &runs,
        &mut run,
        Event::PlanApproved {
            approval_sha256: "a".repeat(64),
            capability_id: Some("self-improvement-lifecycle".into()),
            review_uid: uid(),
            source_ref: "koolade/self-improvement-e2e".into(),
            destination_ref: "main".into(),
            task_uids: vec![fixture.task.task_uid.clone()],
            owner_task_uid: fixture.task.task_uid.clone(),
            parent_task_uids: vec![fixture.task.task_uid.clone()],
        },
    );
    apply(&runs, &mut run, Event::CloneAllocated { clone_uid: uid() });
    let subject_sha256 = fixture.task.implementation_sha256.clone().unwrap();
    apply(
        &runs,
        &mut run,
        Event::ImplementationFinished {
            head_sha: fixture.reviewed_head.clone(),
            subject_sha256: subject_sha256.clone(),
        },
    );
    apply(
        &runs,
        &mut run,
        Event::CodeReviewPassed {
            receipt_uid: fixture
                .task
                .code_review
                .as_ref()
                .unwrap()
                .receipt_uid
                .clone(),
            subject_sha256: subject_sha256.clone(),
        },
    );
    apply(
        &runs,
        &mut run,
        Event::QaPassed {
            receipt_uid: fixture
                .task
                .runtime_qa
                .as_ref()
                .unwrap()
                .receipt_uid
                .clone(),
            subject_sha256: subject_sha256.clone(),
        },
    );
    apply(
        &runs,
        &mut run,
        Event::SpecReconciled {
            receipt_uid: fixture
                .task
                .spec_reconciliation
                .as_ref()
                .unwrap()
                .receipt_uid
                .clone(),
            subject_sha256,
        },
    );
    apply(
        &runs,
        &mut run,
        Event::PrPrepared {
            work_item_uid: fixture.work_uid.clone(),
        },
    );
    assert_eq!(run.phase, Phase::PrPending);
    run_uid
}

fn apply(runs: &SelfImprovementStore<'_>, run: &mut SelfImprovementRunV1, event: Event) {
    let phase = run.phase;
    let event_debug = format!("{event:?}");
    *run = runs
        .apply_event(&run.run_uid, run.revision, &uid(), event)
        .unwrap_or_else(|error| panic!("{event_debug} from {phase:?} failed: {error}"));
}

fn uid() -> String {
    uuid::Uuid::new_v4().hyphenated().to_string()
}
