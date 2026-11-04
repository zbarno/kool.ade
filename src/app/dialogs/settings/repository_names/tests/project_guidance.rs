use super::*;

#[test]
fn failed_private_settings_validation_does_not_persist_guidance() {
    let _shield =
        crate::core::gitops::test_support::shield("guidance-save-after-shared-validation");
    let root = fixture();
    let state_root = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", state_root.path());
    let mut project = test_support::project_from(&root);
    let mut dialog = DlgProjectSettings::from_project(&project);
    dialog.agent_guidance = "Approved project convention.".into();
    dialog.approve_agent_guidance = true;
    dialog.harness_routes.insert(
        "unsupported-role".into(),
        crate::persistence::harness_settings::WorkRoute {
            harness: "codex".into(),
            model: None,
        },
    );

    assert!(dialog.apply(&mut project).is_err());
    assert_eq!(
        crate::persistence::project_guidance::load_for_root(&root).unwrap(),
        None
    );
    test_support::remove_planning_store_for(&root);
    let _ = std::fs::remove_dir_all(root);
}

// Production boundary: verifies that if approved guidance cannot be saved,
// the already-saved private project route state is reported with its local
// revision and guidance remains unavailable. Synthetic local state only; no
// Docker, provider, network, or credentials; setup is low.
#[test]
fn guidance_write_failure_reports_prior_local_state_revision() {
    let _shield = crate::core::gitops::test_support::shield("guidance-save-failure");
    let root = fixture();
    let state_root = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", state_root.path());
    let mut project = test_support::project_from(&root);
    let initial_head = super::git_head(&root);
    let mut dialog = DlgProjectSettings::from_project(&project);
    dialog.agent_guidance = "Approved project convention.".into();
    dialog.approve_agent_guidance = true;
    let existing_manager = dialog
        .harness_routes
        .get(crate::persistence::harness_settings::MANAGER)
        .map(|route| route.harness.as_str());
    let selected_manager = if existing_manager == Some("codex") {
        "claude"
    } else {
        "codex"
    };
    dialog.harness_routes.insert(
        crate::persistence::harness_settings::MANAGER.into(),
        crate::persistence::harness_settings::WorkRoute {
            harness: selected_manager.into(),
            model: None,
        },
    );
    assert_ne!(dialog.harness_routes, project.state.harness_routes.routes);
    let guidance_project_id =
        crate::persistence::dependency_authorization::project_id(&root).unwrap();
    let blocked_state_root = state_root.path().join("projects").join(guidance_project_id);
    std::fs::create_dir_all(blocked_state_root.parent().unwrap()).unwrap();
    std::fs::write(&blocked_state_root, "not a directory").unwrap();

    let error = dialog.apply(&mut project).unwrap_err().detail();
    assert!(
        error.contains("Private project settings were saved at local state revision"),
        "unexpected settings save error: {error}"
    );
    assert!(
        error.contains("approved guidance was not saved"),
        "unexpected settings save error: {error}"
    );
    assert!(
        error.contains("Could not save approved project guidance")
            && (error.contains("Not a directory") || error.contains("File exists")),
        "unexpected guidance persistence failure: {error}"
    );
    let reported_revision = error
        .split("local state revision ")
        .nth(1)
        .and_then(|suffix| suffix.split_once(',').map(|(revision, _)| revision))
        .expect("partial-save message includes the private local revision");
    assert_eq!(reported_revision.len(), 7);
    assert_eq!(
        super::git_head(&root),
        initial_head,
        "private project state must not create Git history"
    );
    assert_eq!(
        project.state.harness_routes.routes[crate::persistence::harness_settings::MANAGER].harness,
        selected_manager
    );
    assert_eq!(
        std::fs::read_to_string(blocked_state_root).unwrap(),
        "not a directory"
    );
    test_support::remove_planning_store_for(&root);
    let _ = std::fs::remove_dir_all(root);
}
