//! Production heading geometry: natural title at left, usable close at right.
//! No GPU or services; setup is one context and four layout frames per width.
use super::*;

#[test]
fn task_heading_reserves_the_close_button_at_both_window_widths() {
    for width in [244.0, 1190.0] {
        let ctx = egui::Context::default();
        for step in 0..4 {
            let mut start = 0.0;
            let mut close = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width + 80.0, 200.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    ui.set_width(width);
                    ui.horizontal(|ui| {
                        start = ui.next_widget_position().x;
                        paint(ui, "Task details", width);
                        close = super::super::close_button(ui).rect;
                    });
                },
            );
            output.textures_delta.clear();
            if step >= 2 {
                output.textures_delta.clear();
                let heading = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.text() == "Task details" => {
                            Some(text)
                        }
                        _ => None,
                    })
                    .expect("painted heading");
                assert!(
                    (heading.pos.x - start).abs() < 2.0,
                    "heading must stay left aligned"
                );
                assert!(
                    close.left() >= start + width - 40.0,
                    "close button must stay at the right edge: {close:?}"
                );
                assert!(
                    close.right() <= start + width,
                    "close button must fit the header"
                );
            }
        }
    }
}

// Exercises the actual modal heading layout; no GPU or external services.
// A string-only unit test cannot establish wrapping or close-button geometry.
#[test]
fn long_question_heading_wraps_without_eliding_text() {
    let title = "Which behavior should the application use when the selected project has unresolved questions and needs the user's decision before planning can continue?";
    for width in [244.0, 800.0] {
        let ctx = egui::Context::default();
        for _ in 0..4 {
            let mut heading_rect = egui::Rect::NOTHING;
            let mut close_rect = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(Default::default(), |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    heading_rect = paint(ui, title, width).rect;
                    close_rect = super::super::close_button(ui).rect;
                });
            });
            output.textures_delta.clear();
            let heading = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == title => Some(text),
                    _ => None,
                })
                .expect("question heading");
            assert!(!heading.galley.elided);
            assert!(heading.galley.rows.len() > 1);
            assert!(heading_rect.right() <= close_rect.left());
        }
    }
}
