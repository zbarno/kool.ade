use crate::app::{root::KooladeApp, welcome::onboarding::Step};

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
            (text.galley.text() == "Use an existing project")
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
