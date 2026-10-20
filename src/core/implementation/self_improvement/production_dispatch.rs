use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::{
        lifecycle::{FeaturePhase, LifecycleStore, TaskPhase, TaskRecord},
        opportunity::{OpportunitySaveResult, OpportunityStore},
        self_improvement::{SelfImprovementEventV1, SelfImprovementPhaseV1, SelfImprovementStore},
    },
    harness::prompt_catalog::{OpportunityChoice, OpportunityDecisionV1},
};
use std::process::Command;

/// Production boundary: exercise the dispatch authorization hook using persisted
/// plan/reviewer receipts, lifecycle task metadata, and self-improvement state.
/// The opt-in CI branch passes that returned verifier into the app-owned Docker
/// session, proves an authorized command runs in the task clone, then proves run
/// cancellation blocks the next command. A unit test cannot prove this wiring
/// across runner dispatch, session creation, and Docker operations. External
/// dependency: Linux rootless Docker with the pinned Rust image. Expected setup
/// cost: one temporary Git repo, persisted planning/lifecycle records, one task
/// clone, and two short Docker commands.
#[test]
fn production_dispatch_verifier_binds_approved_plan_task_source_and_revocation() {
    let temp = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", temp.path().join("operator-home"));
    let repo = temp.path().join("product");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "--quiet", "--initial-branch=main"]);
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/zbarno/kool.ade",
        ],
    );
    std::fs::write(repo.join("README.md"), "fixture\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@koolade.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture baseline",
        ],
    );
    let source_sha = git(&repo, &["rev-parse", "HEAD"]);

    let planning = PlanningStore::new(
        uuid::Uuid::new_v4(),
        temp.path().join("planning"),
        StoreMode::ManagedLocal,
    );
    crate::artifacts::migration::bootstrap_product_with_store(&repo, &planning, "Auth fixture")
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

    let (snapshot, user_receipt, review_receipt) = approved_plan(&planning);
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

    let mut proposal = opportunity(&workspace_uid, &repository_uid, &source_sha);
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
            self_improvement_run(&decision, &proposal, &feature_uid),
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
        super::verify_plan_binding(
            &planning,
            &mismatched_capability,
            &feature_uid,
            &lifecycle_task_uid,
        )
        .is_err()
    );

    let state_root = temp.path().join("state-root");
    std::fs::create_dir_all(&state_root).unwrap();
    let ticket = format!("{planning_item_uid}.md");
    let allocation_key = super::super::super::key(&ticket);
    let project_slug = crate::persistence::project_slug(&state_root.canonicalize().unwrap());
    let task_repository = super::super::super::task_repository::allocated_path(
        &project_slug,
        &repository_uid,
        &allocation_key,
    )
    .unwrap();
    std::fs::create_dir_all(task_repository.parent().unwrap()).unwrap();
    let clone_output = Command::new("git")
        .args(["clone", "--quiet", "--no-hardlinks", "--"])
        .arg(&repo)
        .arg(&task_repository)
        .output()
        .unwrap();
    assert!(
        clone_output.status.success(),
        "task clone failed: {}",
        String::from_utf8_lossy(&clone_output.stderr)
    );
    git(
        &task_repository,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/zbarno/kool.ade",
        ],
    );
    git(
        &task_repository,
        &[
            "checkout",
            "--quiet",
            "-b",
            "koolade/self-improvement-fixture",
            &source_sha,
        ],
    );
    let implementation = implementation_state(
        &ticket,
        &planning_item_uid,
        &repository_uid,
        &project_slug,
        &source_sha,
        &task_repository,
    );
    let task_state_dir = state_root.join("task-state");
    std::fs::create_dir_all(&task_state_dir).unwrap();
    let state_path = task_state_dir.join("state.json");
    std::fs::write(
        &state_path,
        super::super::super::serialize_state(&implementation).unwrap(),
    )
    .unwrap();
    let metadata = crate::artifacts::task_docs::TaskMetadata::new(
        &crate::domain::ArtifactIdentity {
            display_id: "TASK-001".into(),
            uid: planning_item_uid.clone(),
            parent_uid: Some(uuid::Uuid::new_v4().to_string()),
            title: "Production dispatch".into(),
        },
        "root",
        Vec::new(),
    )
    .unwrap();
    let verifier = super::super::authorize_resolved_task_source(
        &planning,
        &state_root,
        state_path.clone(),
        Some(&planning_item_uid),
        Some(&metadata),
        &implementation,
        None,
    )
    .unwrap()
    .expect("the production dispatch hook must install Evolve authorization");
    let generation = verifier.generation();
    verifier.verify(&generation).unwrap();
    assert_changed_persisted_source_is_rejected(
        &state_path,
        &implementation,
        verifier.as_ref(),
        &generation,
    );
    let source_status = git(&repo, &["status", "--porcelain"]);

    if exercise_authorized_docker_if_enabled(
        &task_repository,
        &project_slug,
        &lifecycle_task_uid,
        &source_sha,
        verifier.clone(),
        &runs,
        &ready,
    ) {
        assert_eq!(git(&repo, &["status", "--porcelain"]), source_status);
        return;
    }

    // The same verifier is checked for every Docker operation. Revoking the
    // persisted run after session creation must deny the next operation.
    let cancelled = runs
        .apply_event(
            &ready.run_uid,
            ready.revision,
            &uuid::Uuid::new_v4().to_string(),
            SelfImprovementEventV1::Cancel,
        )
        .unwrap();
    assert_eq!(cancelled.phase, SelfImprovementPhaseV1::Cancelled);
    assert!(verifier.verify(&generation).is_err());
}

#[path = "production_dispatch/support.rs"]
mod support;
use support::*;
