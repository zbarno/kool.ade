//! Compact worker activity in the task detail modal.
use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    view: &crate::ui::task_detail::ViewModel,
    ticket: &str,
    active: bool,
    activity_path: &mut Option<String>,
) {
    ui.add_space(14.0);
    ui.label(RichText::new("Activity").strong().size(16.0));
    crate::ui::task_activity::graph(ui, &view.activity_samples, view.activity_active, 76.0);
    if let Some(progress) = view.progress.as_ref() {
        ui.label(RichText::new(crate::ui::task_activity::preview(progress)).small());
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(crate::ui::task_activity::timing(progress, active))
                    .small()
                    .weak(),
            );
            if ui.small_button("View all activity").clicked() {
                *activity_path = Some(ticket.to_owned());
            }
        });
    } else {
        ui.label(
            RichText::new("No worker activity recorded yet.")
                .small()
                .weak(),
        );
    }
}
