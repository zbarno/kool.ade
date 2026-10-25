use super::super::lifecycle::RecoveryAction;
use super::super::{Controller, WorkKind};

impl Controller {
    pub(crate) fn paint(
        &mut self,
        ui: &mut egui::Ui,
        prerequisite_id: &str,
    ) -> (bool, Option<String>) {
        if !prerequisite_id.starts_with("setup:images:general:") {
            return (false, None);
        }
        let refresh_scan = self.poll(ui.ctx());
        ui.add_space(crate::ui::theme::spacing::M);
        ui.label(crate::ui::theme::section_heading(
            "Prepare the approved image",
        ));
        ui.label(
            "This downloads the architecture-matched, digest-locked general runtime image if needed, then runs a bounded non-root smoke test with networking disabled. The operation uses the local Docker Engine and does not access project files, run host shell commands, or request administrator privileges.",
        );

        if let Some(kind) = self.work_kind {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(match kind {
                    WorkKind::Preview => "Checking the local Docker Engine and pinned image…",
                    WorkKind::Prepare => "Preparing the pinned image and checking isolation…",
                    WorkKind::Cancel => "Updating setup history…",
                });
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        } else if let Some(preview) = &self.preview {
            let request = preview.show_confirmation(ui);
            let cancel = ui.button("Cancel review").clicked();
            if cancel {
                self.cancel_review();
            }
            if let Some(request) = request.filter(|_| !cancel) {
                let task_id = request.proposal().prerequisite_id.clone();
                self.pending_approval = Some(request);
                return (refresh_scan, Some(task_id));
            }
            return (refresh_scan, None);
        } else {
            if let Some(result) = &self.result {
                match result {
                    Ok(()) => ui.colored_label(
                        crate::ui::theme::SUCCESS,
                        "Passed: the approved runtime image is ready. A new setup check will verify it.",
                    ),
                    Err(error) => ui.colored_label(crate::ui::theme::DANGER, error),
                };
            }
            if let Some(action) = self.recovery_action {
                let label = match action {
                    RecoveryAction::Cancel => "Retry saving cancellation",
                    RecoveryAction::Fail(_) => "Retry saving repair outcome",
                    RecoveryAction::ReconcileVerification => "Retry checking repair history",
                };
                if ui.button(label).clicked() {
                    self.begin_recovery(action);
                }
            }
            ui.label(
                "Choose Fix for me to review a proposed repair. Nothing runs until you approve it.",
            );
        }
        (refresh_scan, None)
    }
}
