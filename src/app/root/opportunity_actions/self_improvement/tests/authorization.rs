use super::*;

#[cfg(unix)]
#[test]
fn product_pursuit_records_authenticated_github_user_as_actor() {
    use std::os::unix::fs::PermissionsExt;

    // Production boundary: a product Pursue receipt must use the authenticated
    // GitHub account, not a user-editable display name. The helper unit test
    // cannot prove the app identity probe contract. Uses a local `gh` script;
    // no network or Docker, negligible setup.
    let directory = tempfile::tempdir().unwrap();
    let gh = directory.path().join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\n[ \"$1\" = api ] && [ \"$2\" = user ] || exit 2\nprintf '987654321\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert_eq!(
        super::super::decision_user_id(true, "Editable display name", &gh).unwrap(),
        "github:987654321"
    );
    assert_eq!(
        super::super::decision_user_id(false, "Ada", &gh).unwrap(),
        "Ada"
    );
}

#[cfg(unix)]
#[test]
fn product_pursuit_without_github_auth_is_not_recorded_as_a_decision() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let gh = directory.path().join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();

    assert!(super::super::decision_user_id(true, "Ada", &gh).is_err());
}

#[cfg(unix)]
#[test]
fn app_does_not_record_product_pursuit_when_github_auth_fails() {
    use std::os::unix::fs::PermissionsExt;

    // Production boundary: a failed identity probe during the actual board
    // Pursue action must leave the proposal undecided and must not start its
    // run. A helper unit test cannot prove the UI command stops before saving
    // the receipt. Uses a local failing `gh`, headless egui, temporary stores;
    // no network or Docker.
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    let directory = tempfile::tempdir().unwrap();
    let bin = directory.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(&gh, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let original_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::iter::once(bin)
        .chain(std::env::split_paths(&original_path))
        .collect::<Vec<_>>();
    environment.set("PATH", std::env::join_paths(path).unwrap());

    let mut app = crate::app::root::board_tests::fixture();
    let opportunity_uid = {
        let crate::app::root::Screen::Connected(project) = &mut app.screen else {
            panic!("fixture should be connected")
        };
        app_product_proposal(project)
    };
    let context = egui::Context::default();
    click_text(&mut app, &context, "Improvement proposals (1)");
    click_text(&mut app, &context, "Improve planning reliability");
    click_text(&mut app, &context, "Pursue");

    let crate::app::root::Screen::Connected(project) = &app.screen else {
        panic!("fixture should remain connected")
    };
    let opportunities = OpportunityStore::new(&project.state.planning_store);
    assert!(
        opportunities
            .decision_for_opportunity(&opportunity_uid)
            .unwrap()
            .is_none()
    );
    let (proposal, _) = opportunities.load(&opportunity_uid).unwrap();
    assert_eq!(proposal.status, OpportunityStatusV1::AwaitingDecision);
    assert!(proposal.related_feature_uids.is_empty());
    assert!(app.toasts.last_message().is_some_and(|message| {
        message.contains("Authenticate with GitHub before pursuing a Kool.ad/e self-improvement")
    }));
}
