use super::*;

#[path = "tests/atomic.rs"]
mod atomic;
use crate::{
    app::root::{KooladeApp, Screen},
    artifacts::planning_store::PlanningRoot,
    core::lifecycle::{
        FeatureGoal, FeaturePhase, LifecycleStore, SpecDeltaClassificationV1, SpecDeltaRecordV1,
        SpecReconciliationStatusV1, SpecRevisionDraftV1,
    },
    domain::ArtifactIdentity,
    ui::ApplicationCommand,
};

struct Proposal {
    feature_uid: String,
    requirement_id: String,
    old: String,
    new: String,
    sequence: u64,
}

fn seed_proposal(app: &mut KooladeApp) -> Proposal {
    let Screen::Connected(project) = &mut app.screen else {
        panic!("connected fixture")
    };
    let raw = "# CHG-921: Feature target fixture\n\n## Intent\n\nKeep the fixture feature clear.\n\n## Current Behavior\n\nThe fixture has no target.\n\n## Desired Behavior\n\nThe fixture target is explicit.\n\n## Scope\n\n- target wording\n\n## Requirements\n\n- REQ-001: Original target wording.\n- REQ-002: Secondary target wording.\n\n## Acceptance Criteria\n\n1. The target wording is visible.\n";
    let markdown =
        ArtifactIdentity::preserve_markdown(raw, None, "CHG-921", "Feature target fixture")
            .unwrap();
    let identity = ArtifactIdentity::from_markdown(&markdown).unwrap().unwrap();
    let contract = crate::core::workflow::feature_contract(&markdown);
    let snapshot = crate::core::lifecycle::PlanSnapshotV1::from_feature(
        "CHG-921", &identity, &markdown, &contract,
    )
    .unwrap();
    let store = &project.state.planning_store;
    crate::artifacts::migration::bootstrap_product_with_store(
        &project.state.repo_root,
        store,
        "Feature target fixture",
    )
    .unwrap();
    let (snapshot_path, snapshot_bytes, snapshot_check) =
        crate::core::lifecycle::plan_snapshot::record_change(store, &snapshot)
            .unwrap()
            .unwrap();
    store
        .transaction_with_record_revisions(&[(snapshot_path, snapshot_bytes)], &[snapshot_check])
        .unwrap();

    let documents = crate::artifacts::product_docs::load_documents(store)
        .unwrap()
        .unwrap();
    let document = &documents[0];
    let old = "REQ-001: Original target wording.".to_owned();
    let new = "REQ-001: Adopted target wording.".to_owned();
    let content = format!(
        "{}\n\n{old}\nREQ-002: Secondary target wording.\n",
        document.content
    );
    let path = store
        .planning_layout()
        .product_root()
        .join(&document.module.path)
        .strip_prefix(store.planning_layout().root())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    store
        .transaction(&[(path, content.into_bytes())], None)
        .unwrap();
    project
        .state
        .active_features
        .push(("CHG-921".into(), markdown));

    let feature = FeatureGoal {
        schema_version: 1,
        feature_uid: identity.uid.clone(),
        original_intent: "Feature target fixture".into(),
        draft: "The fixture target is explicit".into(),
        scope: vec!["target wording".into()],
        non_goals: Vec::new(),
        revision: 0,
        phase: FeaturePhase::Exploring,
        approved_plan_sha256: None,
        plan_snapshot_uid: Some(snapshot.uid),
        plan_snapshot_sha256: Some(snapshot.content_hash),
        unresolved_questions: Vec::new(),
        decision_refs: Vec::new(),
        review_refs: Vec::new(),
        readiness_evidence_refs: Vec::new(),
        task_uids: Vec::new(),
    };
    let lifecycle = LifecycleStore::new(store);
    lifecycle
        .create_feature(&feature, &uuid::Uuid::new_v4().hyphenated().to_string())
        .unwrap();
    let feature = lifecycle.load_feature(&identity.uid).unwrap();
    let origin = lifecycle
        .enqueue_manual_spec_refresh(&identity.uid, "feature-decision-fixture")
        .unwrap();
    let hash = crate::core::lifecycle::product_spec::current_product_hash(store).unwrap();
    lifecycle
        .append_spec_revision(
            &identity.uid,
            0,
            SpecRevisionDraftV1 {
                no_change_reason: None,
                content_hash: hash,
                source_revision: feature.revision,
                produced_by_run_id: Some(uuid::Uuid::new_v4().hyphenated().to_string()),
                origin_events: vec![origin.uid.clone()],
                decision_refs: Vec::new(),
                delta_approval_refs: Vec::new(),
                approved_target_hash: None,
                implementation_head_sha: None,
                reconciliation_status: SpecReconciliationStatusV1::NeedsHumanDecision,
                outcome_reason: Some("Proposed target update requires a user decision.".into()),
                deltas: vec![SpecDeltaRecordV1 {
                    requirement_id: "REQ-001".into(),
                    event_id: origin.uid.clone(),
                    old_target: Some(old.clone()),
                    proposed_target: Some(new.clone()),
                    observed_current: None,
                    classification: SpecDeltaClassificationV1::UserDecision,
                    evidence_refs: vec![format!("spec-trigger:{}", origin.uid)],
                    decision_refs: Vec::new(),
                    test_refs: Vec::new(),
                    affected_task_ids: Vec::new(),
                    needs_user_reapproval: true,
                    followup_ids: Vec::new(),
                    operator_decision: None,
                }],
            },
        )
        .unwrap();
    Proposal {
        feature_uid: identity.uid,
        requirement_id: "REQ-001".into(),
        old,
        new,
        sequence: 1,
    }
}

#[test]
// Production boundary: app command dispatch and file-backed lifecycle storage.
// A validator unit test cannot prove the operator command changes canonical
// product prose and the approved target through the controller path. Uses a
// local synthetic project fixture; no external service is required.
fn production_command_dispatch_adopts_proposal_and_updates_revision_and_prose() {
    let mut app = crate::app::root::board_tests::fixture();
    let proposal = seed_proposal(&mut app);
    app.dispatch_ui_command(ApplicationCommand::DecideFeatureSpecDelta {
        feature_uid: proposal.feature_uid.clone(),
        requirement_id: proposal.requirement_id,
        expected_sequence: proposal.sequence,
        adopt: true,
    });
    let Screen::Connected(project) = &app.screen else {
        panic!("connected fixture")
    };
    let store = &project.state.planning_store;
    let current = LifecycleStore::new(store)
        .load_spec_revision(&proposal.feature_uid)
        .unwrap()
        .unwrap();
    let history = LifecycleStore::new(store)
        .spec_revision_history(&proposal.feature_uid)
        .unwrap();
    let (_, deltas) = history.last().unwrap();
    let rendered = crate::artifacts::product_docs::render_product(store)
        .unwrap()
        .unwrap();
    assert!(rendered.contains(&proposal.new));
    assert!(!rendered.contains(&proposal.old));
    assert_eq!(current.seq, proposal.sequence + 1);
    assert_eq!(
        current.approved_target_hash.as_deref(),
        Some(current.content_hash.as_str())
    );
    assert!(deltas[0].operator_decision.is_some());
    let authorization = LifecycleStore::new(store)
        .load_feature_delta_authorization(
            &deltas[0].operator_decision.as_ref().unwrap().receipt_uid,
        )
        .unwrap();
    assert_eq!(authorization.feature_uid, proposal.feature_uid);
    assert_eq!(authorization.source_spec_sequence, proposal.sequence);
    assert_eq!(authorization.requirement_id, deltas[0].requirement_id);
    assert_eq!(authorization.actor, crate::core::lifecycle::GateActor::User);
    assert_eq!(
        authorization.resulting_product_sha256.as_deref(),
        Some(current.content_hash.as_str())
    );
    assert_eq!(current.implementation_head_sha, None);
}

#[test]
// Production boundary: app command dispatch and file-backed lifecycle storage.
// This verifies declining records the operator result without changing target
// prose; direct store tests cannot cover controller routing. Local fixture only.
fn production_command_dispatch_declines_without_changing_target_or_product() {
    let mut app = crate::app::root::board_tests::fixture();
    let proposal = seed_proposal(&mut app);
    let Screen::Connected(project) = &app.screen else {
        panic!("connected fixture")
    };
    let before = crate::artifacts::product_docs::render_product(&project.state.planning_store)
        .unwrap()
        .unwrap();
    let before_target = LifecycleStore::new(&project.state.planning_store)
        .load_spec_revision(&proposal.feature_uid)
        .unwrap()
        .unwrap()
        .approved_target_hash;
    app.dispatch_ui_command(ApplicationCommand::DecideFeatureSpecDelta {
        feature_uid: proposal.feature_uid.clone(),
        requirement_id: proposal.requirement_id,
        expected_sequence: proposal.sequence,
        adopt: false,
    });
    let Screen::Connected(project) = &app.screen else {
        panic!("connected fixture")
    };
    let store = &project.state.planning_store;
    let current = LifecycleStore::new(store)
        .load_spec_revision(&proposal.feature_uid)
        .unwrap()
        .unwrap();
    let after = crate::artifacts::product_docs::render_product(store)
        .unwrap()
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(current.approved_target_hash, before_target);
    assert_eq!(current.seq, proposal.sequence + 1);
}

#[path = "tests/aggregate.rs"]
mod aggregate;
#[path = "tests/optional_text.rs"]
mod optional_text;

#[path = "tests/sequential.rs"]
mod sequential;
