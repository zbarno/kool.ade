use super::*;

const DETAILS_SELECTION_PREFIX: &str = "setup-notice:";

#[derive(Default)]
pub(crate) struct WelcomeSetupAction {
    pub retry: bool,
    pub fix_task: Option<String>,
}

pub(crate) fn paint_board(
    ui: &mut egui::Ui,
    tasks: &[crate::persistence::setup_tasks::LocalTask],
    notices: &[crate::app::setup_attention::SetupIssue],
    checking: bool,
    selected: &mut Option<String>,
) -> WelcomeSetupAction {
    let mut action = WelcomeSetupAction::default();
    let active = tasks
        .iter()
        .filter(|task| task.is_active())
        .collect::<Vec<_>>();
    let notices = visible_notices(tasks, notices);
    let no_active_tasks = active.is_empty();
    let verified = tasks
        .iter()
        .filter(|task| task.state == crate::persistence::setup_tasks::State::Verified)
        .count();
    let total = active.len() + verified;

    ui.add_space(18.0);
    ui.separator();
    ui.add_space(8.0);
    ui.heading("Machine setup");
    ui.label(crate::ui::theme::helper_text(
        "These checks belong to this machine and stay available before you connect a workspace.",
    ));
    if checking {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Checking this machine. You can keep using this screen.");
        });
    } else if total > 0 {
        ui.label(format!(
            "Setup progress · {verified} of {total} checks verified"
        ));
        if no_active_tasks && notices.is_empty() {
            ui.label("The latest setup checks passed. Run them again after a machine change.");
        }
    } else {
        ui.label("Setup progress · no machine checks have completed yet");
    }

    for task in active {
        let key = task.id.as_str();
        let open = super::task_cards::board_card(ui, key, None, false, true, false, |ui| {
            let blocked = !task.blocked_by.is_empty()
                || task.state == crate::persistence::setup_tasks::State::Unsupported;
            crate::ui::theme::badge(
                ui,
                if blocked {
                    "Blocked"
                } else {
                    task.state_label()
                },
                if blocked {
                    crate::ui::theme::WARNING.gamma_multiply(0.14)
                } else {
                    crate::ui::theme::PANEL_ALT
                },
                if blocked {
                    crate::ui::theme::WARNING
                } else {
                    crate::ui::theme::TEXT
                },
            );
            ui.label(egui::RichText::new(crate::ui::layout::setup_task_title(task)).strong());
            ui.add(egui::Label::new(crate::ui::layout::setup_task_summary(task)).truncate());
            for prerequisite_id in &task.blocked_by {
                ui.label(crate::ui::theme::helper_text(format!(
                    "Waiting for {}",
                    human_prerequisite(tasks, prerequisite_id)
                )));
            }
            if task.state == crate::persistence::setup_tasks::State::Unsupported {
                ui.colored_label(
                    crate::ui::theme::WARNING,
                    "Unavailable on this platform. Coding tasks remain disabled.",
                );
            }
            ui.horizontal_wrapped(|ui| {
                if supports_repair(task)
                    && ui
                        .add_enabled(
                            !checking,
                            egui::Button::new("Fix for me").fill(crate::ui::theme::ACTION),
                        )
                        .clicked()
                {
                    *selected = Some(task.id.clone());
                    action.fix_task = Some(task.id.clone());
                }
                if ui.button("Guide me").clicked() {
                    *selected = Some(task.id.clone());
                }
                if ui
                    .add_enabled(!checking, egui::Button::new("Check again"))
                    .clicked()
                {
                    action.retry = true;
                }
            });
        });
        if open {
            *selected = Some(task.id.clone());
        }
    }

    for notice in &notices {
        let selected_key = notice_selection_key(notice.id);
        let open =
            super::task_cards::board_card(ui, &selected_key, None, false, true, false, |ui| {
                crate::ui::theme::badge(
                    ui,
                    "Setup check needs attention",
                    crate::ui::theme::WARNING.gamma_multiply(0.14),
                    crate::ui::theme::WARNING,
                );
                ui.label(egui::RichText::new(notice.title).strong());
                ui.add(egui::Label::new(&notice.issue).truncate());
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Guide me").clicked() {
                        *selected = Some(selected_key.clone());
                    }
                    if ui
                        .add_enabled(!checking, egui::Button::new("Check again"))
                        .clicked()
                    {
                        action.retry = true;
                    }
                });
            });
        if open {
            *selected = Some(selected_key);
        }
    }

    if no_active_tasks && total == 0 && notices.is_empty() && !checking {
        ui.label(
            "No setup problems need your attention. Choose Check again to refresh this result.",
        );
    }
    if ui
        .add_enabled(!checking, egui::Button::new("Check all setup again"))
        .clicked()
    {
        action.retry = true;
    }
    action
}

pub(crate) fn paint_details(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    tasks: &[crate::persistence::setup_tasks::LocalTask],
    notices: &[crate::app::setup_attention::SetupIssue],
    checking: bool,
    selected: &str,
) -> bool {
    let bounds = ui.ctx().content_rect();
    let board = crate::ui::planning_board::ViewModel {
        setup_tasks: tasks.to_vec(),
        setup_attention: notices.to_vec(),
        setup_checking: checking,
        ..Default::default()
    };
    if let Some(task) = tasks.iter().find(|task| task.id == selected) {
        return crate::ui::overlays::show_task_modal(ui, &task.id, bounds, |ui| {
            super::task_details::paint_machine_setup(ui, surface, &board, task)
        });
    }
    let Some(notice_id) = selected.strip_prefix(DETAILS_SELECTION_PREFIX) else {
        return true;
    };
    let Some(notice) = visible_notices(tasks, notices)
        .into_iter()
        .find(|notice| notice.id == notice_id)
    else {
        return true;
    };
    crate::ui::overlays::show_task_modal(ui, notice_id, bounds, |ui| {
        super::task_details::paint_setup_issue(ui, surface, &board, notice)
    })
}

fn visible_notices<'a>(
    tasks: &[crate::persistence::setup_tasks::LocalTask],
    notices: &'a [crate::app::setup_attention::SetupIssue],
) -> Vec<&'a crate::app::setup_attention::SetupIssue> {
    let active_tasks = tasks
        .iter()
        .filter(|task| task.is_active())
        .collect::<Vec<_>>();
    notices
        .iter()
        .filter(|issue| !crate::app::setup_attention::has_specific_task(&active_tasks, issue))
        .collect()
}

pub(crate) fn notice_selection_key(notice_id: &str) -> String {
    format!("{DETAILS_SELECTION_PREFIX}{notice_id}")
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

fn human_prerequisite(tasks: &[crate::persistence::setup_tasks::LocalTask], id: &str) -> String {
    tasks
        .iter()
        .find(|task| task.id == id)
        .map(|task| crate::ui::layout::setup_task_title(task).to_owned())
        .unwrap_or_else(|| "another setup step".into())
}
