use super::super::super::super::task_cards;
use super::super::super::super::*;

pub(in crate::ui::layout::board::columns) fn setup(
    ui: &mut egui::Ui,
    _s: &mut dyn Surface,
    _board: &crate::ui::planning_board::ViewModel,
    issue: &crate::app::setup_attention::SetupIssue,
    selected: &mut Option<String>,
) {
    let open_details = task_cards::board_card(ui, issue.id, None, false, true, false, |ui| {
        super::super::attention::badge(ui, super::super::attention::Kind::WaitingOnUser);
        if ui
            .add(
                egui::Button::new(RichText::new(issue.title).strong())
                    .frame(false)
                    .wrap(),
            )
            .clicked()
        {
            *selected = Some(issue.id.to_owned());
        }
        ui.add(egui::Label::new(&issue.issue).truncate());
        ui.label(theme::helper_text("Setup needs your attention"));
        if ui.button("Open details").clicked() {
            *selected = Some(issue.id.to_owned());
        }
    });
    if open_details {
        *selected = Some(issue.id.to_owned());
    }
}

pub(in crate::ui::layout::board::columns) fn machine_setup(
    ui: &mut egui::Ui,
    task: &crate::persistence::setup_tasks::LocalTask,
    selected: &mut Option<String>,
) {
    let key = task.id.as_str();
    let open_details = task_cards::board_card(ui, key, None, false, true, false, |ui| {
        let blocked = matches!(
            task.state,
            crate::persistence::setup_tasks::State::BlockedByPrerequisite
                | crate::persistence::setup_tasks::State::Unsupported
        );
        super::super::attention::badge(
            ui,
            if blocked {
                super::super::attention::Kind::Blocked
            } else {
                super::super::attention::Kind::WaitingOnUser
            },
        );
        if ui
            .add(
                egui::Button::new(RichText::new(&task.title).strong())
                    .frame(false)
                    .wrap(),
            )
            .clicked()
        {
            *selected = Some(task.id.clone());
        }
        ui.add(egui::Label::new(&task.explanation).truncate());
        ui.label(theme::helper_text(format!(
            "Machine setup · {:?}",
            task.state
        )));
        if ui.button("Open details").clicked() {
            *selected = Some(task.id.clone());
        }
    });
    if open_details {
        *selected = Some(task.id.clone());
    }
}
