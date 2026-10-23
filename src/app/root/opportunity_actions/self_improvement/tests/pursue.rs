use super::*;

/// Production boundary: a board Pursue action creates a planning run while
/// leaving implementation dispatch untouched. Helper/store tests cannot prove
/// the UI action wires both services in the right order. Uses headless egui and
/// a local Git fixture; no Docker or network, low setup cost.
#[test]
fn pursue_button_creates_a_planning_run_without_dispatching_implementation() {
    let mut app = crate::app::root::board_tests::fixture();
    let uid = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        app_proposal(project, OpportunityTargetKindV1::ActiveUserProject)
    };
    let ctx = egui::Context::default();
    click_text(&mut app, &ctx, "Improvement proposals (1)");
    click_text(&mut app, &ctx, "Improve planning reliability");
    click_text(&mut app, &ctx, "Pursue");
    assert!(
        app.toasts
            .last_message()
            .is_some_and(|message| message.contains("Draft feature")),
        "Pursue should confirm that a run was created, got: {:?}",
        app.toasts.last_message()
    );

    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let opportunities = OpportunityStore::new(&project.state.planning_store);
    let decision = opportunities
        .decision_for_opportunity(&uid)
        .unwrap()
        .expect("Pursue action persists its operator receipt");
    assert_eq!(decision.decision, OpportunityDecisionKindV1::Pursue);
    let (proposal, _) = opportunities.load(&uid).unwrap();
    assert_eq!(
        proposal.status,
        OpportunityStatusV1::Planning,
        "Pursue action did not link the draft feature: {:?}",
        app.toasts.last_message()
    );
    assert!(decision.user_id.contains("Koolade Acceptance"));
    assert_eq!(
        proposal.target.kind,
        OpportunityTargetKindV1::ActiveUserProject
    );
    assert!(project.active_implementations.is_empty());
    assert!(project.active_turn.is_none());
}

#[cfg(unix)]
#[test]
fn product_pursuit_action_persists_authenticated_actor_and_creates_only_a_plan() {
    use std::os::unix::fs::PermissionsExt;

    // Production boundary: exercise the app opportunity controller through
    // decision persistence and run creation. Store-only tests cannot prove the
    // authenticated CLI identity reaches the saved actor. Uses a headless app,
    // temporary Git/store fixture, and fake gh; no network or Docker, moderate
    // setup.
    let mut app = crate::app::root::board_tests::fixture();
    let uid = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        app_proposal(project, OpportunityTargetKindV1::KooladeProduct)
    };
    let gh_dir = tempfile::tempdir().unwrap();
    let gh = gh_dir.path().join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\n[ \"$1\" = api ] && [ \"$2\" = user ] || exit 2\nprintf '987654321\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let revision = OpportunityStore::new(&project.state.planning_store)
        .load(&uid)
        .unwrap()
        .1;

    app.decide_opportunity_with_gh(&uid, revision, OpportunityChoice::Pursue, None, &gh);

    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let opportunities = OpportunityStore::new(&project.state.planning_store);
    let decision = opportunities
        .decision_for_opportunity(&uid)
        .unwrap()
        .expect("authenticated Pursue decision should persist");
    assert_eq!(decision.user_id, "github:987654321");
    let run_uid = SelfImprovementStore::run_uid_for_decision(&decision.uid).unwrap();
    let run = SelfImprovementStore::new(&project.state.planning_store)
        .load(&run_uid)
        .unwrap();
    assert_eq!(run.phase, SelfImprovementPhaseV1::Planning);
    assert!(run.clone_uid.is_none());
    assert!(project.active_implementations.is_empty());
    assert!(project.active_turn.is_none());
}

#[cfg(unix)]
#[test]
fn product_pursuit_action_does_not_persist_when_github_authentication_fails() {
    use std::os::unix::fs::PermissionsExt;

    let mut app = crate::app::root::board_tests::fixture();
    let uid = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        app_proposal(project, OpportunityTargetKindV1::KooladeProduct)
    };
    let gh_dir = tempfile::tempdir().unwrap();
    let gh = gh_dir.path().join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let revision = OpportunityStore::new(&project.state.planning_store)
        .load(&uid)
        .unwrap()
        .1;

    app.decide_opportunity_with_gh(&uid, revision, OpportunityChoice::Pursue, None, &gh);

    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let opportunities = OpportunityStore::new(&project.state.planning_store);
    assert!(
        opportunities
            .decision_for_opportunity(&uid)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        opportunities.load(&uid).unwrap().0.status,
        OpportunityStatusV1::AwaitingDecision
    );
}
