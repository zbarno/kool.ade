//! Measured worker activity and a compact output preview.
use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    view: &crate::ui::task_detail::ViewModel,
    ticket: &str,
    active: bool,
    activity_path: &mut Option<String>,
) {
    ui.add_space(10.0);
    egui::Frame::NONE
        .fill(theme::BG)
        .corner_radius(10)
        .stroke(egui::Stroke::new(1.0, theme::BORDER_STRONG))
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Activity")
                        .strong()
                        .size(16.0)
                        .color(theme::BLUE_BRIGHT),
                );
                if view.progress.is_some() {
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("View all activity").clicked() {
                            *activity_path = Some(ticket.to_owned());
                        }
                    });
                }
            });
            crate::ui::task_activity::graph(ui, &view.activity_samples, view.activity_active, 48.0);
            if let Some(progress) = view.progress.as_ref() {
                let preview = crate::ui::task_activity::preview(progress);
                let mut job = egui::text::LayoutJob::simple(
                    preview.clone(),
                    egui::FontId::proportional(12.5),
                    theme::TEXT_DIM,
                    ui.available_width(),
                );
                job.wrap.max_rows = 2;
                let text = ui.painter().layout_job(job);
                ui.add(egui::Label::new(text)).on_hover_text(preview);
                ui.label(
                    RichText::new(crate::ui::task_activity::timing(progress, active))
                        .size(11.5)
                        .color(theme::TEXT_DIM),
                );
            } else {
                ui.label(
                    RichText::new("No worker activity recorded yet.")
                        .size(12.0)
                        .color(theme::TEXT_DIM),
                );
            }
        });
}
