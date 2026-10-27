use super::*;

mod conversation;
mod guide;
mod history;

#[cfg(test)]
mod tests;

pub(super) fn machine_setup_summary(
    task: &crate::persistence::setup_tasks::LocalTask,
) -> &'static str {
    guide::summary(task)
}

pub(super) fn machine_setup_title(task: &crate::persistence::setup_tasks::LocalTask) -> String {
    guide::plain_title(task)
}

pub(super) fn paint_machine_setup(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    task: &crate::persistence::setup_tasks::LocalTask,
) {
    let host = crate::app::setup_attention::catalog::HostKind::current();
    ui.heading(guide::plain_title(task));
    ui.horizontal_wrapped(|ui| {
        state_badge(ui, task);
        ui.label(crate::ui::theme::metadata_text(platform_label(host)));
    });
    ui.add_space(crate::ui::theme::spacing::M);

    if uses_split_panes(ui.available_width()) {
        ui.columns(2, |columns| {
            guide::paint(&mut columns[0], task, host);
            details(&mut columns[1], surface, board, task, host);
        });
    } else {
        guide::paint(ui, task, host);
        ui.separator();
        details(ui, surface, board, task, host);
    }
}

fn uses_split_panes(width: f32) -> bool {
    width >= 1000.0
}

pub(super) fn paint_setup_issue(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    issue: &crate::app::setup_attention::SetupIssue,
) {
    ui.heading(issue.title);
    ui.label(crate::ui::theme::metadata_text(
        "Setup check needs your input",
    ));
    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("What needs attention"));
    ui.label(&issue.issue);
    ui.collapsing("Why this matters", |ui| ui.label(issue.why));
    ui.label(crate::ui::theme::section_heading("Recommended next step"));
    ui.label(issue.recommendation);
    ui.label(crate::ui::theme::helper_text(issue.impact));
    ui.label(issue.next_action);
    if ui.button("Open workspace settings").clicked() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("koolade_workspace_settings_open"), true);
        });
    }
    let label = if board.setup_checking {
        "Checking setup…"
    } else {
        "Check again"
    };
    if ui
        .add_enabled(!board.setup_checking, egui::Button::new(label))
        .clicked()
    {
        surface.dispatch(crate::ui::ApplicationCommand::RetrySetupCheck);
    }
}

fn details(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    board: &crate::ui::planning_board::ViewModel,
    task: &crate::persistence::setup_tasks::LocalTask,
    host: crate::app::setup_attention::catalog::HostKind,
) {
    ui.label(crate::ui::theme::section_heading("Setup status"));
    ui.label(guide::status_message(task.state));
    if !task.blocked_by.is_empty() {
        ui.add_space(crate::ui::theme::spacing::S);
        ui.label(crate::ui::theme::section_heading("Waiting for"));
        for prerequisite_id in &task.blocked_by {
            ui.label(prerequisite_title(board, prerequisite_id));
        }
    }

    ui.add_space(crate::ui::theme::spacing::M);
    ui.label(crate::ui::theme::section_heading("Next step"));
    ui.label(&task.next_step);
    ui.add_space(crate::ui::theme::spacing::M);
    paint_evidence(ui, task);
    history::paint(ui, task);
    history::technical(ui, task);
    conversation::paint(ui, surface, task);

    ui.add_space(crate::ui::theme::spacing::M);
    ui.horizontal_wrapped(|ui| {
        if supports_repair(task, surface.supports_provider_install(&task.id))
            && ui
                .add_enabled(
                    !board.setup_checking,
                    egui::Button::new("Fix for me").fill(crate::ui::theme::ACTION),
                )
                .clicked()
        {
            surface.dispatch(crate::ui::ApplicationCommand::RequestSetupTaskFix {
                task_id: task.id.clone(),
            });
        }
        let label = if board.setup_checking {
            "Checking setup…"
        } else {
            "Check again"
        };
        if ui
            .add_enabled(!board.setup_checking, egui::Button::new(label))
            .clicked()
        {
            surface.dispatch(crate::ui::ApplicationCommand::RetrySetupCheck);
        }
        if task.id.starts_with("setup:cli:") && ui.button("Open Coding Tools").clicked() {
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("koolade_open_coding_settings"), true);
            });
        }
    });
    if should_paint_image_preparation(task) {
        surface.setup_image_preparation(ui, &task.id);
    }
    if should_paint_claude_install(task, surface.supports_provider_install(&task.id)) {
        surface.setup_provider_install(ui, &task.id);
    }
    if task.state == crate::persistence::setup_tasks::State::Unsupported {
        ui.add_space(crate::ui::theme::spacing::S);
        ui.colored_label(crate::ui::theme::WARNING, unsupported_message(host));
    }
}

fn paint_evidence(ui: &mut egui::Ui, task: &crate::persistence::setup_tasks::LocalTask) {
    ui.label(crate::ui::theme::section_heading("Verification"));
    if let Some(result) = &task.verified_result {
        ui.colored_label(
            crate::ui::theme::SUCCESS,
            format!(
                "Confirmed by {} on check {}",
                result.evidence_domain, result.scan_generation
            ),
        );
    } else if task.verified_at_unix_ms.is_some() {
        ui.label("A previous check passed; the current setup still needs verification.");
    } else {
        ui.label("This setup step has not passed a current verification check.");
    }
}

fn prerequisite_title(board: &crate::ui::planning_board::ViewModel, id: &str) -> String {
    board
        .setup_tasks
        .iter()
        .find(|task| task.id == id)
        .map(|task| crate::ui::layout::setup_task_title(task).to_owned())
        .unwrap_or_else(|| guide::humanize_prerequisite(id))
}

fn supports_repair(
    task: &crate::persistence::setup_tasks::LocalTask,
    provider_install_supported: bool,
) -> bool {
    (is_supported_general_image(task)
        || (is_claude_install_task(task) && provider_install_supported))
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

fn is_claude_install_task(task: &crate::persistence::setup_tasks::LocalTask) -> bool {
    task.id == "setup:cli:claude:install"
}

fn should_paint_claude_install(
    task: &crate::persistence::setup_tasks::LocalTask,
    provider_install_supported: bool,
) -> bool {
    use crate::persistence::setup_tasks::State;
    is_claude_install_task(task)
        && task.blocked_by.is_empty()
        && provider_install_supported
        && matches!(
            task.state,
            State::ReadyToFix
                | State::NeedsApproval
                | State::InProgress
                | State::AwaitingVerification
                | State::NeedsVerification
                | State::Failed
                | State::Paused
                | State::Interrupted
                | State::Cancelled
        )
}

fn should_paint_image_preparation(task: &crate::persistence::setup_tasks::LocalTask) -> bool {
    use crate::persistence::setup_tasks::State;
    is_supported_general_image(task)
        && (matches!(
            task.state,
            State::NeedsApproval | State::InProgress | State::AwaitingVerification
        ) || supports_repair(task, false))
}

fn is_supported_general_image(task: &crate::persistence::setup_tasks::LocalTask) -> bool {
    crate::app::bootstrap_repair_policy::parse_locked_image_task_id(&task.id).is_some()
}

fn state_badge(ui: &mut egui::Ui, task: &crate::persistence::setup_tasks::LocalTask) {
    let state = task.state;
    use crate::persistence::setup_tasks::State;
    let color = match state {
        State::Verified => crate::ui::theme::SUCCESS,
        State::Unsupported | State::Failed | State::Interrupted => crate::ui::theme::WARNING,
        State::BlockedByPrerequisite | State::NeedsApproval | State::Paused => {
            crate::ui::theme::WARNING
        }
        _ => crate::ui::theme::TEXT_DIM,
    };
    crate::ui::theme::badge(ui, task.state_label(), color.gamma_multiply(0.14), color);
}

fn platform_label(host: crate::app::setup_attention::catalog::HostKind) -> &'static str {
    use crate::app::setup_attention::catalog::HostKind;
    match host {
        HostKind::Linux => "Linux · native rootless engine when supported",
        HostKind::MacOs => "macOS · Docker Desktop Linux VM",
        HostKind::Windows => "Windows · Docker Desktop Linux backend",
        HostKind::Unsupported => "This operating system is not supported for coding tasks",
    }
}

fn unsupported_message(host: crate::app::setup_attention::catalog::HostKind) -> &'static str {
    use crate::app::setup_attention::catalog::HostKind;
    match host {
        HostKind::MacOs => {
            "Docker Desktop on macOS is detected, but coding-task execution is not release-certified on this host."
        }
        HostKind::Windows => {
            "Docker Desktop and WSL 2 setup can be checked, but coding-task execution is not release-certified on this host."
        }
        HostKind::Linux => {
            "This setup path is unavailable on this Linux configuration. It will not be treated as ready."
        }
        HostKind::Unsupported => "Kool.ad/e does not enable coding tasks on this operating system.",
    }
}
