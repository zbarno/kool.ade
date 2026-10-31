use super::super::support::{StateHome, frame, type_into, visible_text};
use crate::app::root::{KooladeApp, Screen, WelcomeEntry, WizField};

/// Boundary: welcome wizard UI -> managed workspace creation -> connection.
/// This proves the real user flow stores planning outside the code checkout;
/// a state-only test cannot prove the filesystem roots stay separate. Uses
/// temporary local Git repositories, no provider, Docker, or network.
#[test]
fn blank_project_wizard_creates_and_opens_separate_code_and_planning_repos() {
    let _identity =
        crate::core::gitops::test_support::shield_with_synthetic_identity("blank-project-wizard");
    let base = std::env::temp_dir().join(format!(
        "koolade-blank-wizard-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    let parent = base.join("projects");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&parent).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);

    let ctx = egui::Context::default();
    let mut app = KooladeApp::default();
    app.wizard_open(WelcomeEntry::BlankProject);
    type_into(
        &mut app,
        &ctx,
        "Shown on the board and planning repository",
        "Acme Portal",
    );
    app.wiz_apply_field(WizField::Parent, parent.to_string_lossy().into_owned());
    app.wizard_continue();
    let review = visible_text(&frame(&mut app, &ctx, Vec::new()));
    assert!(
        review.iter().any(|line| line.contains("Step 2 of 2")),
        "a spaced title should advance to review: {review:?}"
    );
    app.wizard_commit();

    let Screen::Connected(project) = &app.screen else {
        let visible = visible_text(&frame(&mut app, &ctx, Vec::new()));
        panic!("blank wizard failed; visible: {visible:?}");
    };
    let code = parent.join("acme-portal");
    let planning = project.state.planning_store.root.clone();
    assert!(code.join(".git").is_dir());
    assert!(planning.join("manifest.json").is_file());
    assert!(planning.join("planning/product/index.md").is_file());
    assert_ne!(
        code.canonicalize().unwrap(),
        planning.canonicalize().unwrap()
    );
    assert!(!parent.join("acme-portal.packet").exists());
    assert!(!code.join(".koolade-packet").exists());
    assert_eq!(project.state.repositories.repositories.len(), 1);
    assert_eq!(
        crate::core::managed_workspace::WorkspaceManager::from_state_root()
            .list_local()
            .unwrap()
            .len(),
        1
    );
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}
