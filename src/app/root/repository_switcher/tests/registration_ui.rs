use super::*;

fn frame(app: &mut KooladeApp, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        ctx.style_mut_of(theme, |style| style.animation_time = 0.0);
    }
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1800.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            crate::ui::layout::paint(ui, app);
            app.render_queued_settings_dialog(ui);
        },
    );
    output.textures_delta.clear();
    output
}

fn click_text(app: &mut KooladeApp, ctx: &egui::Context, label: &str) -> egui::FullOutput {
    let output = frame(app, ctx, Vec::new());
    let position = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
            }
            _ => None,
        })
        .or_else(|| {
            output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().contains(label) => {
                    Some(text.pos + text.galley.mesh_bounds.center().to_vec2())
                }
                _ => None,
            })
        })
        .unwrap_or_else(|| panic!("missing UI text {label}"));
    frame(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
    );
    frame(
        app,
        ctx,
        vec![egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    frame(app, ctx, Vec::new())
}

fn register_repository_from_settings(
    app: &mut KooladeApp,
    ctx: &egui::Context,
    id: &str,
    role: &str,
    remote: &str,
    checkout: &std::path::Path,
) {
    click_text(app, ctx, "Register another code repository…");
    let mut dialog = match app.dialog.take().unwrap() {
        crate::app::root::Dialog::AddCodeRepository(dialog) => dialog,
        _ => panic!("the repository form should be open"),
    };
    dialog.id = id.into();
    dialog.role = role.into();
    dialog.remote = remote.into();
    dialog.checkout = checkout.to_string_lossy().into_owned();
    app.dialog = Some(crate::app::root::Dialog::AddCodeRepository(dialog));
    click_text(app, ctx, "Register");
    assert!(
        app.dialog.is_none(),
        "successful registration closes the form"
    );
}

#[test]
fn managed_workspace_menu_registers_a_code_repository_and_local_checkout() {
    let _shield = crate::core::gitops::test_support::shield_with_synthetic_identity(
        "register-code-repository-ui",
    );
    let base = std::env::temp_dir().join(format!(
        "koolade_registration_ui_{}_{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&base).unwrap();
    let koolade_home = base.join("koolade-home");
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", &koolade_home);

    let manager = crate::core::managed_workspace::WorkspaceManager::from_state_root();
    let workspace = manager.create("Registration UI project").unwrap();
    let planning_remote = base.join("planning.git");
    git(
        &base,
        &[
            "init",
            "--quiet",
            "--bare",
            "--initial-branch=main",
            planning_remote.to_str().unwrap(),
        ],
    );
    manager
        .set_planning_remote(workspace.id, &planning_remote.to_string_lossy())
        .unwrap();
    manager.sync_planning(workspace.id).unwrap();
    let project = crate::app::welcome::attempt_open_workspace(workspace.id).unwrap();
    assert_eq!(
        project.state.planning_store.mode,
        crate::artifacts::planning_store::StoreMode::ManagedShared
    );
    assert!(project.state.repositories.repositories.is_empty());
    let checkout = base.join("api-service");
    let api_remote = "https://example.test/team/api-service.git";
    init_repo(&checkout, api_remote);
    let frontend_checkout = base.join("web-client");
    let frontend_remote = "https://example.test/team/web-client.git";
    init_repo(&frontend_checkout, frontend_remote);
    let mut app = KooladeApp {
        screen: Screen::Connected(Box::new(project)),
        ..Default::default()
    };
    let ctx = egui::Context::default();
    assert!(!crate::ui::Surface::is_git_repo(&app));
    assert!(crate::ui::Surface::repository_branches(&app).is_empty());
    assert_eq!(crate::ui::Surface::default_repository_branch(&app), "");
    let task_form = click_text(&mut app, &ctx, "+ New Task");
    let task_text = task_form
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(task_text.contains("You can plan without a code checkout"));
    assert!(!task_text.contains("Source Branch"));
    click_text(&mut app, &ctx, "Cancel");

    click_text(&mut app, &ctx, "Workspace");
    click_text(&mut app, &ctx, "Settings…");
    click_text(&mut app, &ctx, "Planning Repository");
    register_repository_from_settings(
        &mut app,
        &ctx,
        "api-service",
        "API service",
        api_remote,
        &checkout,
    );
    register_repository_from_settings(
        &mut app,
        &ctx,
        "web-client",
        "Web client",
        frontend_remote,
        &frontend_checkout,
    );

    let Screen::Connected(project) = &app.screen else {
        panic!("registration keeps the planning workspace connected")
    };
    assert_eq!(project.state.repositories.repositories.len(), 2);
    assert!(
        project
            .state
            .repositories
            .repositories
            .iter()
            .any(|repository| {
                repository.id == "api-service" && repository.remote == api_remote
            })
    );
    assert!(
        project
            .state
            .repositories
            .repositories
            .iter()
            .any(|repository| {
                repository.id == "web-client" && repository.remote == frontend_remote
            })
    );
    assert_eq!(
        project
            .state
            .repositories
            .target(&project.state.repo_root, "api-service")
            .unwrap(),
        checkout.canonicalize().unwrap()
    );
    assert_eq!(
        project
            .state
            .repositories
            .target(&project.state.repo_root, "web-client")
            .unwrap(),
        frontend_checkout.canonicalize().unwrap()
    );
    assert!(!project.state.repo_root.join(".koolade-packet").exists());
    assert_eq!(manager.list_local().unwrap().len(), 1);

    drop(environment);
    let _ = std::fs::remove_dir_all(base);
}
