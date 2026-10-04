use super::*;

pub(super) fn paint(ui: &mut egui::Ui, doc: &crate::artifacts::task_docs::TaskDocument) {
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        theme::badge(ui, "TASK", theme::ACCENT_SOFT, theme::BLUE_BRIGHT);
        ui.add(
            egui::Label::new(
                RichText::new(task_key(&doc.path))
                    .monospace()
                    .size(11.0)
                    .color(theme::TEXT_DIM),
            )
            .truncate(),
        )
        .on_hover_text(&doc.path);
    });
    let title = super::super::board::presentation::human_title(&doc.title, &doc.path);
    let mut job = egui::text::LayoutJob::simple(
        title.to_owned(),
        egui::FontId::proportional(if ui.available_width() < 500.0 {
            20.0
        } else {
            24.0
        }),
        theme::TEXT,
        ui.available_width(),
    );
    job.wrap.max_rows = if ui.available_width() < 500.0 { 4 } else { 3 };
    let text = ui.painter().layout_job(job);
    ui.add(egui::Label::new(text)).on_hover_text(title);
    ui.add_space(4.0);
}
