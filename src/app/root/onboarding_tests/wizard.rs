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
