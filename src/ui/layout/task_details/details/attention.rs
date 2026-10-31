use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    brief: Option<&crate::core::attention::Brief>,
    _decision_sent: bool,
    failure: Option<&str>,
    checks_unavailable: bool,
) {
    if let Some(brief) = brief {
        for step in &brief.steps {
            ui.add(egui::Label::new(format!("{}: {}", step.owner, step.action)).wrap());
        }
    } else if checks_unavailable {
        ui.label("Kool.ad/e couldn't complete the project check. Your work is safe and hasn't been published.");
    } else if let Some(error) = failure {
        let actions = reply::failure_actions(error);
        if actions.is_empty() {
            ui.label(
                "Review the blocker and reply in the conversation. Resume after it is resolved.",
            );
        } else {
            for action in actions {
                ui.add(egui::Label::new(format!("• {action}")).wrap());
            }
        }
    } else {
        ui.label("Start implementation when this task is ready.");
    }
}
