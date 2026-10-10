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

#[test]
fn managed_workspace_menu_registers_a_code_repository_and_local_checkout() {
    let _shield = crate::core::gitops::test_support::shield("register-code-repository-ui");
    let base = std::env::temp_dir().join(format!(
        "koolade_registration_ui_{}_{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    std::fs::create_dir_all(&base).unwrap();
    let koolade_home = base.join("koolade-home");
    let previous = std::env::var_os("KOOLADE_HOME");
    // SAFETY: the Git test shield serializes KOOLADE_HOME changes.
    unsafe { std::env::set_var("KOOLADE_HOME", &koolade_home) };
    let _restore = KooladeHome(previous);

    let manager = crate::core::managed_workspace::WorkspaceManager::from_state_root();
    let workspace = manager.create("Registration UI project").unwrap();
    let project = crate::app::welcome::attempt_open_workspace(workspace.id).unwrap();
    assert!(project.state.repositories.repositories.is_empty());
    let checkout = base.join("api-service");
    init_repo(&checkout, "https://example.test/team/api-service.git");
    let mut app = KooladeApp {
        screen: Screen::Connected(Box::new(project)),
        ..Default::default()
    };
    let ctx = egui::Context::default();

    click_text(&mut app, &ctx, "Workspace");
    click_text(&mut app, &ctx, "Registered repositories");
    click_text(&mut app, &ctx, "Add code repository…");
    assert!(matches!(
        app.dialog,
        Some(crate::app::root::Dialog::AddCodeRepository(_))
    ));

    let mut dialog = match app.dialog.take().unwrap() {
        crate::app::root::Dialog::AddCodeRepository(dialog) => dialog,
        _ => panic!("the repository form should be open"),
    };
    dialog.id = "api-service".into();
    dialog.role = "API service".into();
    dialog.remote = "https://example.test/team/api-service.git".into();
    dialog.checkout = checkout.to_string_lossy().into_owned();
    app.dialog = Some(crate::app::root::Dialog::AddCodeRepository(dialog));
    click_text(&mut app, &ctx, "Register");

    assert!(
        app.dialog.is_none(),
        "successful registration closes the form; feedback: {:?}",
        match &app.dialog {
            Some(crate::app::root::Dialog::AddCodeRepository(dialog)) => {
                dialog.feedback.as_ref()
            }
            _ => None,
        }
    );
    let Screen::Connected(project) = &app.screen else {
        panic!("registration keeps the planning workspace connected")
    };
    assert!(
        project
            .state
            .repositories
            .repositories
            .iter()
            .any(|repository| repository.id == "api-service")
    );
    assert_eq!(
        project
            .state
            .repositories
            .target(&project.state.repo_root, "api-service")
            .unwrap(),
        checkout.canonicalize().unwrap()
    );
    assert!(!project.state.repo_root.join(".koolade-packet").exists());

    drop(_restore);
    let _ = std::fs::remove_dir_all(base);
}
