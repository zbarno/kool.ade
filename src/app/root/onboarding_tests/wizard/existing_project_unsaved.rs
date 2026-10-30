use super::paint;
use crate::{
    app::{
        root::{KooladeApp, Screen},
        welcome::onboarding::Step,
    },
    core::managed_workspace::{ExistingProject, ExistingProjectKind, WorkspaceManager},
};

#[test]
fn existing_planning_pair_opens_through_wizard_with_unsaved_setup_state() {
    use super::super::support::StateHome;

    // Production boundary: explicit pinned UI action through local Git
    // attachment and workspace loading, even when navigation state cannot be
    // saved. Local Git only; two small repositories and no network/Docker/SSH.
    // A unit test cannot prove the complete consent-to-Connected path.
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("application-state");
    let repos = temp.path().join("repos");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&repos).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);

    let code = repos.join("kool.ade");
    std::fs::create_dir(&code).unwrap();
    let init = std::process::Command::new("git")
        .args([
            "-c",
            "init.templateDir=",
            "init",
            "--quiet",
            "--initial-branch=main",
            "--",
        ])
        .arg(&code)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .status()
        .unwrap();
    assert!(init.success(), "initialize synthetic code repository");
    let commit = std::process::Command::new("git")
        .args([
            "-C",
            code.to_str().unwrap(),
            "-c",
            "user.name=Example Developer",
            "-c",
            "user.email=developer@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Synthetic existing code",
        ])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .status()
        .unwrap();
    assert!(commit.success(), "commit synthetic code repository");

    let manager = WorkspaceManager::from_state_root();
    let planning = manager.create("Synthetic planning").unwrap();
    let request = ExistingProject {
        folder: code.canonicalize().unwrap(),
        planning_repo: Some(planning.planning_repo.clone()),
        workspace_id: planning.id,
        repository_id: "synthetic-code".into(),
        title: "Koolade".into(),
        create_workspace: false,
    };
    let mut app = KooladeApp::default();
    app.onboarding.step = Step::ExistingProject;
    app.onboarding.existing = Some(request);
    app.onboarding_project_kind = Some(ExistingProjectKind::Tracked);
    app.onboarding.error = Some("saved progress is unavailable".into());
    let ctx = egui::Context::default();
    paint(&mut app, &ctx, vec![]);
    let output = paint(&mut app, &ctx, vec![]);
    let position = output
        .shapes
        .iter()
        .find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            (text.galley.text() == "Connect and open project")
                .then(|| text.pos + text.galley.mesh_bounds.center().to_vec2())
        })
        .expect("existing pair has a visible primary action");
    for pressed in [true, false] {
        paint(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    assert!(
        app.onboarding_creation.is_some()
            || app.onboarding_open.is_some()
            || matches!(app.screen, Screen::Connected(_)),
        "pinned action should start the attachment"
    );
    for _ in 0..1000 {
        paint(&mut app, &ctx, vec![]);
        if matches!(app.screen, Screen::Connected(_)) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let Screen::Connected(project) = &app.screen else {
        panic!(
            "existing pair should open; connection error: {:?}, attachment pending: {}, open pending: {}",
            app.conn_error,
            app.onboarding_creation.is_some(),
            app.onboarding_open.is_some(),
        )
    };
    assert_eq!(project.state.planning_store.project_id, planning.id);
    assert_eq!(project.state.repo_root, planning.planning_repo);
}
