use crate::app::{root::KooladeApp, welcome::onboarding::Step};

#[path = "wizard/existing_project_action.rs"]
mod existing_project_action;

fn paint(app: &mut KooladeApp, ctx: &egui::Context, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(360.0, 640.0),
            )),
            events,
            ..Default::default()
        },
        |ui| app.paint_screen(ui),
    );
    output.textures_delta.clear();
    output
}

#[test]
fn narrow_first_run_get_started_opens_manager_step() {
    // Production egui event boundary at narrow width; no GPU, network, Docker
    // or provider needed. A state-only test cannot prove button routing.
    let mut app = KooladeApp::default();
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
            (text.galley.text() == "Get started")
                .then(|| text.pos + text.galley.mesh_bounds.center().to_vec2())
        })
        .expect("first launch offers Get started");
    assert!(position.x >= 0.0 && position.x <= 360.0 && position.y <= 640.0);
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
    assert_eq!(app.onboarding.step, Step::Manager);
    assert_eq!(app.onboarding.revision, 1);
}

#[test]
fn existing_project_button_routes_to_folder_review() {
    // Production click routing, no folder picker or services. A direct enum
    // assertion would miss a button accidentally wired to Advanced.
    let mut app = KooladeApp::default();
    app.onboarding.step = Step::Projects;
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
            (text.galley.text() == "Open an existing code project…")
                .then(|| text.pos + text.galley.mesh_bounds.center().to_vec2())
        })
        .unwrap();
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
    assert_eq!(app.onboarding.step, Step::ExistingProject);
}

#[test]
fn folder_inspection_discards_results_for_a_different_request() {
    use crate::core::managed_workspace::{ExistingProject, ExistingProjectKind};
    let mut app = KooladeApp::default();
    let request = ExistingProject {
        folder: "/synthetic/current-project".into(),
        planning_repo: None,
        workspace_id: uuid::Uuid::new_v4(),
        repository_id: "current".into(),
        title: "Current project".into(),
        create_workspace: true,
    };
    app.onboarding.existing = Some(request.clone());
    app.apply_onboarding_inspection((
        "previous".into(),
        request.folder.clone(),
        Ok(ExistingProjectKind::Tracked),
    ));
    assert!(app.onboarding_project_kind.is_none());
    app.apply_onboarding_inspection((
        request.repository_id.clone(),
        "/synthetic/previous-project".into(),
        Err("stale error".into()),
    ));
    assert!(app.conn_error.is_none());
    app.apply_onboarding_inspection((
        request.repository_id,
        request.folder,
        Ok(ExistingProjectKind::NeedsHistory),
    ));
    assert_eq!(
        app.onboarding_project_kind,
        Some(ExistingProjectKind::NeedsHistory)
    );
}

#[test]
fn narrow_help_escape_preserves_step_and_keyboard_navigation_activates_start() {
    // Real egui key events and focus traversal; no GPU or external services.
    // A state-only test would miss inaccessible controls or consumed key events.
    let mut app = KooladeApp::default();
    let ctx = egui::Context::default();
    let key = |key, shift| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            shift,
            ..Default::default()
        },
    };
    paint(&mut app, &ctx, vec![]);
    paint(&mut app, &ctx, vec![key(egui::Key::Tab, false)]);
    let first = ctx
        .memory(|memory| memory.focused())
        .expect("Tab focuses a control");
    paint(&mut app, &ctx, vec![key(egui::Key::Tab, false)]);
    let second = ctx
        .memory(|memory| memory.focused())
        .expect("Tab reaches next control");
    assert_ne!(first, second);
    paint(&mut app, &ctx, vec![key(egui::Key::Tab, true)]);
    // egui defers reverse traversal one frame so gained_focus is observable.
    paint(&mut app, &ctx, vec![]);
    assert_eq!(ctx.memory(|memory| memory.focused()), Some(first));
    app.onboarding.help_open = true;
    let revision = app.onboarding.revision;
    paint(&mut app, &ctx, vec![]);
    paint(&mut app, &ctx, vec![key(egui::Key::Escape, false)]);
    assert!(!app.onboarding.help_open);
    assert_eq!(app.onboarding.step, Step::Start);
    assert_eq!(app.onboarding.revision, revision);
    paint(&mut app, &ctx, vec![key(egui::Key::Tab, false)]);
    paint(&mut app, &ctx, vec![key(egui::Key::Tab, false)]);
    paint(&mut app, &ctx, vec![key(egui::Key::Enter, false)]);
    assert_eq!(app.onboarding.step, Step::Manager);
}

#[test]
fn editing_review_location_preserves_original_request_without_creating_files() {
    // Production review UI to private transaction-state boundary. No Git or
    // provider dependency; temporary paths only. Pure path tests cannot prove
    // unapplied edits disable creation or preserve the original request.
    let root = tempfile::tempdir().unwrap();
    let original = crate::core::managed_workspace::ApplicationCreation {
        transaction_id: uuid::Uuid::new_v4(),
        workspace_id: uuid::Uuid::new_v4(),
        title: "Garden".into(),
        intent: "Track plants".into(),
        code_directory: root.path().join("garden"),
    };
    let mut app = KooladeApp::default();
    app.onboarding.step = Step::ReviewProject;
    app.onboarding.creation = Some(original.clone());
    app.onboarding.project_directory_name = "garden-notes".into();
    let ctx = egui::Context::default();
    paint(&mut app, &ctx, vec![]);
    let output = paint(&mut app, &ctx, vec![]);
    let text = super::support::visible_text(&output);
    assert!(!text.iter().any(|text| text == "Approve and create project"));
    assert_eq!(app.onboarding.creation, Some(original.clone()));
    assert!(app.onboarding_creation.is_none());
    let position = output
        .shapes
        .iter()
        .find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else {
                return None;
            };
            (text.galley.text() == "Use this folder name")
                .then(|| text.pos + text.galley.mesh_bounds.center().to_vec2())
        })
        .unwrap();
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
    let changed = app.onboarding.creation.as_ref().unwrap();
    assert_ne!(changed.transaction_id, original.transaction_id);
    assert_ne!(changed.workspace_id, original.workspace_id);
    assert_eq!(changed.code_directory, root.path().join("garden-notes"));
    assert_eq!(app.onboarding.previous_creations, vec![original]);
    assert!(app.onboarding_creation.is_none());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
