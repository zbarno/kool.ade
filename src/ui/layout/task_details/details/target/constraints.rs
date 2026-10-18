use crate::{
    execution::contract::TargetPreference,
    ui::{Surface, task_detail},
};

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    ticket: &str,
    view: &task_detail::ViewModel,
) {
    ui.collapsing("Allowed targets for this task", |ui| {
        let choices = view
            .target_choices
            .iter()
            .filter(|choice| !matches!(choice.preference, TargetPreference::Auto))
            .collect::<Vec<_>>();
        let mut allowed = if view.task_allowed_targets.is_empty() {
            choices
                .iter()
                .map(|choice| choice.preference.clone())
                .collect::<Vec<_>>()
        } else {
            view.task_allowed_targets.clone()
        };
        for choice in choices {
            let mut checked = allowed.contains(&choice.preference);
            let response = ui
                .add_enabled(
                    allowed.len() > 1 || checked,
                    egui::Checkbox::new(&mut checked, &choice.label),
                )
                .on_hover_text(&choice.reason);
            if response.changed() {
                if checked {
                    allowed.push(choice.preference.clone());
                } else {
                    allowed.retain(|target| target != &choice.preference);
                }
                surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                    task_detail::Command::SetTaskAllowedTargets {
                        ticket: ticket.to_owned(),
                        targets: allowed.clone(),
                    },
                ));
            }
        }
        if !view.task_allowed_targets.is_empty()
            && ui.button("Clear task target constraints").clicked()
        {
            surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                task_detail::Command::SetTaskAllowedTargets {
                    ticket: ticket.to_owned(),
                    targets: Vec::new(),
                },
            ));
        }
        ui.label(egui::RichText::new("An empty saved list means any trusted target is allowed. These local task constraints apply to explicit choices and Automatic routing.").small().weak());
    });
}
