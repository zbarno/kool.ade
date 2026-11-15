use super::*;

#[test]
fn ready_plan_yields_an_in_review_card_that_survives_reload_as_a_noop() {
    let repo = repo("ready");
    let mut project = seed(&repo, &feature_uid(), true);
    assert_eq!(project.planning_work.len(), 1, "root only before sync");
    let root_uid = project.planning_work[0].uid.clone();

    let effects = sync(&mut project);
    assert!(
        effects.evaluations_wanted.is_empty(),
        "ready plans do not request evaluation turns"
    );
    assert!(
        !effects.announcements.is_empty(),
        "the new ready card is told to the user"
    );
    let card = the_card(&project);
    let card_uid = card.uid.clone();
    let payload = card.plan_confirmation.as_ref().expect("typed payload");
    let hash = payload.snapshot_content_hash.clone();
    let parent = card.parent_uid.clone();
    let ready_state = card.status;
    assert_eq!(ready_state, WorkStatus::InReview);
    assert!(payload.ready, "readiness comes from typed evidence only");
    assert!(
        payload.cause.is_none() && payload.blockers.is_empty(),
        "ready card names no blockers"
    );
    assert_eq!(
        parent,
        Some(root_uid),
        "card is owned by the TaskIntent root"
    );
    // Durability: reload from the private store and demand strict no-op.
    let mut reloaded = project_from(&repo.root);
    reloaded.planning_work = crate::core::planning_work::load(&reloaded.state.planning_store)
        .expect("work records load");
    let reloaded_card = the_card(&reloaded);
    assert_eq!(reloaded_card.uid, card_uid);
    assert_eq!(reloaded_card.status, WorkStatus::InReview);
    let reloaded_hash = reloaded_card
        .plan_confirmation
        .as_ref()
        .expect("payload survived")
        .snapshot_content_hash
        .clone();
    assert_eq!(reloaded_hash, hash);
    let before = reloaded.planning_work.clone();
    let quiet = sync(&mut reloaded);
    assert!(quiet.announcements.is_empty(), "no repeat announcements");
    assert!(quiet.evaluations_wanted.is_empty(), "no repeat evaluations");
    assert_eq!(
        reloaded.planning_work, before,
        "second pass is a strict no-op"
    );
    remove_planning_store_for(&repo.root);
}

#[test]
fn unready_plan_creates_needs_attention_and_evaluates_once_per_spec_revision() {
    let repo = repo("unready");
    let mut project = seed(&repo, &feature_uid(), false);
    let effects = sync(&mut project);
    let card = the_card(&project);
    let card_uid = card.uid.clone();
    let card_key = card.key.clone();
    let card_detail = card.detail.trim().to_owned();
    let payload = card.plan_confirmation.as_ref().expect("typed payload");
    let caused = payload.cause;
    assert!(
        !card_detail.is_empty(),
        "concrete copy explains the blockage"
    );
    assert!(!payload.ready);
    assert!(!payload.blockers.is_empty());
    assert!(matches!(
        caused,
        Some(crate::core::planning_work::PlanNotReadyCause::AssessmentNeeded)
    ));
    assert_eq!(
        effects.evaluations_wanted.len(),
        1,
        "one readiness-evaluation turn requested"
    );
    assert_eq!(
        effects.evaluations_wanted[0].work_key, card_key,
        "evaluation binds to the card"
    );
    let quiet = sync(&mut project);
    assert!(
        quiet.evaluations_wanted.is_empty(),
        "the same spec revision is never re-requested"
    );
    assert!(quiet.announcements.is_empty());
    assert_eq!(the_card(&project).uid, card_uid, "card is not rebuilt");
    remove_planning_store_for(&repo.root);
}

#[test]
fn malformed_plan_document_is_contained_without_any_evaluation_request() {
    let repo = repo("malformed");
    let mut project = seed(&repo, &feature_uid(), false);
    // Swap in a document that carries the durable markers but no reviewable
    // spec body: identity parses, the plan does not.
    let uid = project
        .planning_work
        .first()
        .expect("root")
        .feature_uid
        .clone()
        .unwrap();
    project.state.active_features = vec![(
        "FEAT-IT".to_owned(),
        format!(
            "# FEAT-IT: Broken plan\n\n<!-- koolade-artifact-id:v1 {{\"uid\":\"{uid}\",\"displayId\":\"FEAT-IT\",\"title\":\"Broken plan\"}} -->\n\nGarbage body.\n\n<!-- koolade-change:v1 {{\"schemaVersion\":1,\"uid\":\"{uid}\",\"displayId\":\"CHG-IT\",\"status\":\"draft\"}} -->\n"
        ),
    )];
    let effects = sync(&mut project);
    let card = the_card(&project);
    let card_status = card.status;
    let payload = card.plan_confirmation.as_ref().expect("typed payload");
    let caused = payload.cause;
    assert!(!card.detail.trim().is_empty());
    assert_eq!(card_status, WorkStatus::NeedsAttention);
    assert!(matches!(
        caused,
        Some(crate::core::planning_work::PlanNotReadyCause::MalformedDocument)
    ));
    assert!(
        effects.evaluations_wanted.is_empty(),
        "an unparseable plan is repaired by people, not by planner turns"
    );
    remove_planning_store_for(&repo.root);
}

#[test]
fn revised_spec_moves_the_same_card_and_keeps_demanding_a_decision() {
    let repo = repo("drift");
    let uid = feature_uid();
    let mut project = seed(&repo, &uid, true);
    sync(&mut project);
    let card_uid = the_card(&project).uid.clone();
    let first_hash = the_card(&project)
        .plan_confirmation
        .as_ref()
        .expect("payload")
        .snapshot_content_hash
        .clone();
    // Revise the spec: same durable feature identity, new requirement, hence
    // a new immutable snapshot the approver has never seen.
    let revised = spec_body(&uid) + "\n- REQ-002: The saved list shows each view’s name.\n";
    let identity = crate::domain::ArtifactIdentity::from_markdown(&revised)
        .ok()
        .flatten()
        .expect("identity parses");
    let contract = crate::core::workflow::feature_contract(&revised);
    let snapshot = plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        "FEAT-IT",
        &identity,
        &revised,
        &contract,
    )
    .expect("revised snapshot builds");
    assert_ne!(
        snapshot.content_hash, first_hash,
        "the revision must produce a distinct snapshot"
    );
    let store = &project.state.planning_store;
    let (current, current_revision) = QuestionStore::new(store)
        .load_readiness(&uid)
        .expect("readiness loads")
        .expect("prior checklist exists");
    let _ = current;
    let requirements = snapshot
        .requirements
        .iter()
        .map(|requirement| requirement.stable_id.clone())
        .collect::<Vec<_>>();
    let checklist = crate::core::planning::ReadinessChecklistV1 {
        feature_uid: uid.clone(),
        plan_revision: current.plan_revision + 1,
        spec_revision: Some(snapshot.spec_revision.clone()),
        required_requirement_ids: Some(requirements.clone()),
        assessed_axes: all_material_axes(),
        acceptance_tests: requirements
            .iter()
            .map(|requirement| AcceptanceTestTrace {
                requirement_id: requirement.clone(),
                test_ref: format!("test:saved_views_restore_{requirement}"),
            })
            .collect(),
        evidence_refs: Vec::new(),
        unresolved_blocker_ids: Vec::new(),
        blocker_notes: Vec::new(),
        ready: true,
        stalled: false,
        no_progress_count: 0,
        progress_fingerprint: None,
        revision: current_revision,
    };
    QuestionStore::new(store)
        .save_readiness(&checklist, current_revision)
        .expect("revised readiness saves");
    project.state.active_features = vec![("FEAT-IT".to_owned(), revised)];
    let effects = sync(&mut project);
    let card = the_card(&project);
    assert_eq!(
        card.uid, card_uid,
        "revision updates, it does not recreate, the card"
    );
    assert_eq!(
        card.status,
        WorkStatus::InReview,
        "revised plan goes back in front of the decider"
    );
    let payload = card.plan_confirmation.as_ref().expect("payload");
    assert_eq!(payload.snapshot_content_hash, snapshot.content_hash);
    assert_eq!(payload.snapshot_spec_revision, snapshot.spec_revision);
    assert!(
        effects
            .announcements
            .iter()
            .any(|text| text.contains("review") || text.contains("change")),
        "the drift is surfaced, not silent"
    );
    remove_planning_store_for(&repo.root);
}
