use crate::ui::{layout::task_cards, theme};

fn render(ctx: &egui::Context, events: Vec<egui::Event>) -> (egui::Rect, f32, f32, bool, bool) {
    let mut button = egui::Rect::NOTHING;
    let (mut bottom, mut next_top, mut child_clicked, mut card_clicked) = (0.0, 0.0, false, false);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(360.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.set_max_width(240.0);
                card_clicked =
                    task_cards::board_card(ui, "first", None, false, true, false, |ui| {
                        ui.label(
                            "A long decision prompt that must stay inside the card. ".repeat(8),
                        );
                        let response = ui.button("Respond to this task");
                        button = response.rect;
                        child_clicked = response.clicked();
                    });
                bottom = ui.cursor().top();
                task_cards::board_card(ui, "second", None, false, false, false, |ui| {
                    next_top = ui.cursor().top();
                    ui.label("Next task");
                });
            });
        },
    );
    output.textures_delta.clear();
    (button, bottom, next_top, child_clicked, card_clicked)
}

#[test]
fn cards_grow_to_fit_content_and_keep_child_buttons_interactive() {
    let ctx = egui::Context::default();
    theme::apply(&ctx);
    ctx.style_mut_of(egui::Theme::Dark, |style| style.animation_time = 0.0);
    for _ in 0..3 {
        render(&ctx, vec![]);
    }
    let (button, bottom, next_top, _, _) = render(&ctx, vec![]);
    assert!(
        bottom > 200.0,
        "long content should expand past the former fixed height"
    );
    assert!(
        button.bottom() < bottom && next_top >= bottom,
        "cards cannot overlap"
    );
    let pos = button.center();
    let mut child_clicked = false;
    for pressed in [true, false] {
        let (_, _, _, child, card) = render(
            &ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
        child_clicked |= child;
        assert!(!card, "card background must not swallow child clicks");
    }
    assert!(child_clicked, "child action must receive the click");
}
