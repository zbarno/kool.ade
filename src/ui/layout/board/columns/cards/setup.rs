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
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    task: &crate::persistence::setup_tasks::LocalTask,
    selected: &mut Option<String>,
) {
    let key = task.id.as_str();
    let open_details = task_cards::board_card(ui, key, None, false, true, false, |ui| {
        let blocked = !task.blocked_by.is_empty()
            || task.state == crate::persistence::setup_tasks::State::Unsupported;
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
                egui::Button::new(
                    RichText::new(crate::ui::layout::setup_task_title(task)).strong(),
                )
                .frame(false)
                .wrap(),
            )
            .clicked()
        {
            *selected = Some(task.id.clone());
        }
        ui.add(egui::Label::new(crate::ui::layout::setup_task_summary(task)).truncate());
        ui.label(theme::helper_text(format!(
            "Machine setup · {}",
            task.state_label()
        )));
        for prerequisite_id in &task.blocked_by {
            let title = board
                .setup_tasks
                .iter()
                .find(|candidate| candidate.id == *prerequisite_id)
                .map(crate::ui::layout::setup_task_title)
                .unwrap_or_else(|| "another setup step".to_owned());
            ui.label(theme::helper_text(format!("Waiting for {title}")));
        }
        ui.horizontal_wrapped(|ui| {
            if supports_repair(task)
                && ui
                    .add_enabled(
                        !board.setup_checking,
                        egui::Button::new("Fix for me").fill(theme::ACTION),
                    )
                    .clicked()
            {
                *selected = Some(task.id.clone());
                surface.dispatch(crate::ui::ApplicationCommand::RequestSetupTaskFix {
                    task_id: task.id.clone(),
                });
            }
            if ui.button("Guide me").clicked() {
                *selected = Some(task.id.clone());
            }
            let label = if board.setup_checking {
                "Checking…"
            } else {
                "Check again"
            };
            if ui
                .add_enabled(!board.setup_checking, egui::Button::new(label))
                .clicked()
            {
                surface.dispatch(crate::ui::ApplicationCommand::RetrySetupCheck);
            }
        });
        if task.state == crate::persistence::setup_tasks::State::Unsupported {
            ui.colored_label(
                theme::WARNING,
                "Unavailable here. This setup state will not unlock coding tasks.",
            );
        }
    });
    if open_details {
        *selected = Some(task.id.clone());
    }
}

fn supports_repair(task: &crate::persistence::setup_tasks::LocalTask) -> bool {
    task.id
        .strip_prefix("setup:images:general:")
        .is_some_and(|architecture| matches!(architecture, "amd64" | "arm64"))
        && task.blocked_by.is_empty()
        && matches!(
            task.state,
            crate::persistence::setup_tasks::State::ReadyToFix
                | crate::persistence::setup_tasks::State::Failed
                | crate::persistence::setup_tasks::State::Paused
                | crate::persistence::setup_tasks::State::Interrupted
                | crate::persistence::setup_tasks::State::Cancelled
        )
}
