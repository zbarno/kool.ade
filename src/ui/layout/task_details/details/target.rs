use crate::ui::{Surface, task_detail};

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    ticket: &str,
    view: &task_detail::ViewModel,
) {
    ui.group(|ui| {
        ui.label(egui::RichText::new("Execution target").strong());
        let mut selected = view.target_preference.clone();
        let selected_label = view
            .target_choices
            .iter()
            .find(|choice| choice.preference == selected)
            .map(|choice| choice.label.as_str())
            .unwrap_or("Unavailable target");
        egui::ComboBox::from_id_salt(("task_execution_target", ticket))
            .selected_text(selected_label)
            .show_ui(ui, |ui| {
                for choice in &view.target_choices {
                    let response = ui.add_enabled(
                        choice.available,
                        egui::Button::selectable(
                            selected == choice.preference,
                            &choice.label,
                        ),
                    );
                    if response.clicked() {
                        selected = choice.preference.clone();
                    }
                    response.on_hover_text(&choice.reason);
                }
            });
        if selected != view.target_preference {
            surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                task_detail::Command::SetExecutionTarget {
                    ticket: ticket.to_owned(),
                    preference: selected,
                },
            ));
        }
        if let Some(choice) = view
            .target_choices
            .iter()
            .find(|choice| choice.preference == view.target_preference)
        {
            ui.label(egui::RichText::new(&choice.reason).small().weak());
        }
        ui.label(egui::RichText::new("A preference does not grant execution permission. Unavailable or unapproved targets block execution; Kool.ad/e will not silently select another target or run on the host.").small().weak());
    });
}
