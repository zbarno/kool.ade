use crate::ui::theme;
use egui::{Stroke, Ui, vec2};

/// Quiet line icon and concise guidance for a lane with no visible cards.
pub(super) fn paint(ui: &mut Ui, column: usize, headline: &str, detail: &str) {
    ui.add_space(12.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(40.0, 36.0), egui::Sense::hover());
        let painter = ui.painter();
        let center = rect.center();
        let stroke = Stroke::new(1.5, theme::TEXT_MUTED);
        match column {
            0 => {
                painter.circle_stroke(center, 9.0, stroke);
                painter.line_segment([center - vec2(4.0, 0.0), center + vec2(4.0, 0.0)], stroke);
                painter.line_segment([center - vec2(0.0, 4.0), center + vec2(0.0, 4.0)], stroke);
            }
            1 => {
                painter.circle_stroke(center, 9.0, stroke);
                painter.line_segment([center, center - vec2(0.0, 5.0)], stroke);
                painter.line_segment([center, center + vec2(4.0, 2.0)], stroke);
            }
            2 => {
                let lens = center - vec2(2.0, 2.0);
                painter.circle_stroke(lens, 7.0, stroke);
                painter.line_segment([lens + vec2(5.0, 5.0), lens + vec2(11.0, 11.0)], stroke);
            }
            3 => {
                let points = vec![
                    center + vec2(0.0, -10.0),
                    center + vec2(10.0, 8.0),
                    center + vec2(-10.0, 8.0),
                ];
                painter.add(egui::Shape::closed_line(points, stroke));
                painter.line_segment([center, center + vec2(0.0, 3.0)], stroke);
                painter.circle_filled(center + vec2(0.0, 5.5), 0.9, theme::TEXT_MUTED);
            }
            _ => {
                painter.circle_stroke(center, 9.0, stroke);
                painter.add(egui::Shape::line(
                    vec![
                        center + vec2(-4.5, 0.0),
                        center + vec2(-1.0, 3.5),
                        center + vec2(5.0, -4.0),
                    ],
                    stroke,
                ));
            }
        }
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(headline)
                .size(13.0)
                .strong()
                .color(theme::TEXT_DIM),
        );
        ui.add(
            egui::Label::new(
                egui::RichText::new(detail)
                    .size(12.0)
                    .color(theme::TEXT_DIM),
            )
            .wrap(),
        );
    });
}
