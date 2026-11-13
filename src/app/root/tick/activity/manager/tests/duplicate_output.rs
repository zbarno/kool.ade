/// Production boundary: two completed Manager results from stale app snapshots
/// enter consecutive full app activity ticks with the same reviewed finding.
/// Direct disposition-store tests cannot prove the Manager callback rejects
/// the stale second output without creating another action. Uses one temporary
/// Git repo and private PlanningStore; no provider, Docker, SSH, or network.
#[test]
fn duplicate_manager_outputs_create_one_durable_plan_disposition() {
    use std::time::Instant;

    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let home = tempfile::tempdir().unwrap();
    environment.set("KOOLADE_HOME", home.path());
    let chat_slug = format!("manager-duplicate-{}", uuid::Uuid::new_v4());
    let (mut setup, root, _) = crate::app::root::feature_approval_test_support::setup();
    crate::app::root::feature_approval_test_support::record_test_plan_review(
        &mut setup,
        "CHG-004",
        crate::core::lifecycle::GateDecision::Rejected,
    );
    let planning = crate::app::root::feature_approval_test_support::planning_store(&setup);
    let mut first = crate::app::session::test_support::project_from_store(&root, &planning);
    let mut second = crate::app::session::test_support::project_from_store(&root, &planning);
    first.chat_slug = chat_slug.clone();
    second.chat_slug = chat_slug.clone();
    first.activity.manager_history = crate::persistence::manager_activity::load(&chat_slug);
    second.activity.manager_history = first.activity.manager_history.clone();
    let first_output = crate::app::manager::duplicate_disposition_output_for_test(&first);
    let second_output = crate::app::manager::duplicate_disposition_output_for_test(&second);
    let target_revision = first_output.proposed_actions[0].expected_revision.clone();

    first.activity.last_update = Some(Instant::now());
    second.activity.last_update = Some(Instant::now());
    first.activity.manager = Some(crate::app::manager::Manager::completed_for_test(Ok(
        first_output,
    )));
    second.activity.manager = Some(crate::app::manager::Manager::completed_for_test(Ok(
        second_output,
    )));

    let mut app = crate::app::root::board_tests::fixture();
    app.screen = crate::app::root::Screen::Connected(Box::new(first));
    app.advance_activity();
    app.screen = crate::app::root::Screen::Connected(Box::new(second));
    app.advance_activity();
    let crate::app::root::Screen::Connected(second) = app.screen else {
        unreachable!()
    };

    let persisted = crate::core::planning_work::load(&planning).unwrap();
    let dispositions = persisted
        .iter()
        .filter(|work| {
            work.kind == crate::core::planning_work::WorkKind::PlanReviewDisposition
                && work.feature_id.as_deref() == Some("CHG-004")
        })
        .collect::<Vec<_>>();
    assert_eq!(
        dispositions.len(),
        1,
        "duplicate completed Manager outputs leave one durable disposition"
    );
    assert!(
        dispositions[0].key.contains(&target_revision),
        "the surviving action stays bound to the reviewed revision"
    );
    assert!(
        second.activity.manager_history.iter().any(|message| message
            .text
            .contains("The reviewed plan finding needs repair.")),
        "the second Manager result is recorded even though its action is stale"
    );

    std::fs::remove_dir_all(&planning.root).unwrap();
    crate::app::root::feature_approval_test_support::remove_fixture_root(&root);
}
