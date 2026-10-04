use super::*;
#[test]
fn item_modal_grows_when_the_workspace_is_resized() {
    let ctx = egui::Context::default();
    for size in [egui::vec2(1080.0, 640.0), egui::vec2(2560.0, 1440.0)] {
        let bounds = egui::Rect::from_min_max(
            egui::pos2(380.0, 90.0),
            egui::pos2(size.x - 28.0, size.y - 20.0),
        );
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ui| {
                    show_panel_modal(ui, "Resize item", bounds, |ui| {
                        ui.label("Short content");
                    });
                },
            );
            if let Some(rect) = output.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.corner_radius.nw == 12 && r.stroke.width == 1.0 => {
                    Some(r.rect)
                }
                _ => None,
            }) {
                assert!(
                    rect.height() >= bounds.height() - 30.0,
                    "Modal failed to grow: {rect:?}"
                );
                assert!(bounds.contains_rect(rect), "Modal overflow: {rect:?}");
            }
            output.textures_delta.clear();
        }
    }
}

#[test]
fn long_modal_stays_inside_small_and_large_viewports_and_closes_with_escape() {
    for size in [
        egui::vec2(420.0, 320.0),
        egui::vec2(1080.0, 640.0),
        egui::vec2(1480.0, 900.0),
    ] {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        for step in 0..4 {
            let mut closed = false;
            let events = if step == 3 {
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }]
            } else {
                vec![]
            };
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    events,
                    ..Default::default()
                },
                |ui| {
                    closed = show_modal(ui, true, "Large dialog", 860.0, |ui| {
                        for _ in 0..50 {
                            ui.label(
                                "Long content that needs to wrap and scroll within this dialog.",
                            );
                        }
                    });
                },
            );
            if step >= 2 {
                fn panel(shape: &egui::Shape) -> Option<egui::Rect> {
                    match shape {
                        egui::Shape::Rect(rect)
                            if rect.corner_radius.nw == 12 && rect.stroke.width == 1.5 =>
                        {
                            Some(rect.rect)
                        }
                        egui::Shape::Vec(shapes) => shapes.iter().find_map(panel),
                        _ => None,
                    }
                }
                let card = output
                    .shapes
                    .iter()
                    .find_map(|shape| panel(&shape.shape))
                    .expect("modal frame");
                assert!(viewport.contains_rect(card), "{size:?}: {card:?}");
            }
            assert_eq!(closed, step == 3);
            output.textures_delta.clear();
        }
    }
}

#[test]
fn title_drag_moves_modal_and_keeps_close_button_usable() {
    let ctx = egui::Context::default();
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
    let frame = |events| {
        let mut closed = false;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(viewport),
                events,
                ..Default::default()
            },
            |ui| {
                closed = show_modal(ui, true, "Drag me", 320.0, |ui| {
                    ui.label("Modal content");
                });
            },
        );
        output.textures_delta.clear();
        closed
    };
    for _ in 0..3 {
        frame(vec![]);
    }
    let id = egui::Id::new("koolade_modal").with("Drag me");
    let before = ctx.memory(|m| m.area_rect(id)).unwrap();
    let start = before.min + egui::vec2(65.0, 30.0);
    let end = start + egui::vec2(90.0, 45.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    frame(vec![egui::Event::PointerMoved(start), button(start, true)]);
    frame(vec![egui::Event::PointerMoved(end)]);
    frame(vec![button(end, false)]);
    frame(vec![]);
    let after = ctx.memory(|m| m.area_rect(id)).unwrap();
    assert!(
        (after.min - before.min - egui::vec2(90.0, 45.0)).length() < 2.0,
        "{before:?} -> {after:?}"
    );
    assert!(viewport.contains_rect(after));
    let close = egui::pos2(after.right() - 30.0, after.top() + 30.0);
    frame(vec![egui::Event::PointerMoved(close), button(close, true)]);
    assert!(frame(vec![button(close, false)]));
}
