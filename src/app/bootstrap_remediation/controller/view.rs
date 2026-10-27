use egui::Ui;

use super::{Controller, WorkKind};

impl Controller {
    pub(crate) fn paint(&mut self, ui: &mut Ui, prerequisite_id: &str) -> (bool, Option<String>) {
        if !super::proposal::is_supported_task(prerequisite_id) {
            return (false, None);
        }
        let completed = self.poll(ui.ctx());
        ui.add_space(crate::ui::theme::spacing::M);
        ui.label(crate::ui::theme::section_heading(
            "Guided rootless Docker repair",
        ));
        ui.label(
            "Review the exact Linux distribution operation and its effects. Nothing changes until you approve it, and the operation runs only after fresh host checks pass.",
        );

        if let Some(work) = self.work {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(match work {
                    WorkKind::Preview => "Preparing the repair review…",
                    WorkKind::Execute => "Running approved repair and checking readiness…",
                    WorkKind::Cancel => "Saving review cancellation…",
                });
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
            return (completed, None);
        }

        if let Some(preview) = &self.preview {
            let request = preview.show_confirmation(ui);
            let cancel = ui.button("Cancel review").clicked();
            if cancel {
                self.cancel_review();
            }
            if let Some(request) = request.filter(|_| !cancel) {
                let task_id = request.plan.prerequisite_id.clone();
                self.pending_approval = Some(request);
                return (completed, Some(task_id));
            }
            return (completed, None);
        }

        if let Some(result) = &self.result {
            match result {
                Ok(()) => ui.colored_label(
                    crate::ui::theme::SUCCESS,
                    "The approved repair completed. A fresh setup check will verify it.",
                ),
                Err(error) => ui.colored_label(crate::ui::theme::DANGER, error),
            };
        }
        ui.label("Choose Fix for me to review the approved operation. Nothing runs until you approve it.");
        (completed, None)
    }
}
