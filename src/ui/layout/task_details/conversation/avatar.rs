//! Small role icons from the reference conversation layout.
pub(super) fn paint(ui: &mut egui::Ui, user: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 44.0), egui::Sense::hover());
    let center = rect.center();
    let painter = ui.painter();
    let white = super::theme::TEXT;
    painter.circle_filled(
        center,
        20.0,
        if user {
            egui::Color32::from_rgb(37, 80, 124)
        } else {
            egui::Color32::from_rgb(0, 87, 196)
        },
    );
    if user {
        painter.circle_filled(center - egui::vec2(0.0, 5.0), 5.0, white);
        painter.rect_filled(
            egui::Rect::from_center_size(center + egui::vec2(0.0, 7.0), egui::vec2(17.0, 8.0)),
            4,
            white,
        );
    } else {
        let body = egui::Rect::from_center_size(center, egui::vec2(22.0, 16.0));
        painter.rect_stroke(
            body,
            4,
            egui::Stroke::new(2.0, white),
            egui::StrokeKind::Inside,
        );
        for offset in [-4.0, 4.0] {
            painter.circle_filled(center + egui::vec2(offset, 0.0), 2.0, white);
        }
        painter.line_segment(
            [
                center - egui::vec2(0.0, 8.0),
                center - egui::vec2(0.0, 12.0),
            ],
            egui::Stroke::new(2.0, white),
        );
        painter.circle_filled(center - egui::vec2(0.0, 13.0), 2.0, white);
    }
}
