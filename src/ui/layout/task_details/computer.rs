use super::*;

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn Surface, task_id: &str) {
    let sessions = surface.computer_use_task_sessions(task_id);
    egui::CollapsingHeader::new("Computer")
        .id_salt(("task_computer", task_id))
        .default_open(false)
        .show(ui, |ui| {
            if sessions.is_empty() {
                ui.label("No computer session is active for this task.");
                return;
            }
            for session in &sessions {
                paint_session(ui, surface, task_id, session);
                ui.add_space(10.0);
            }
        });
}

fn paint_session(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    task_id: &str,
    session: &crate::computer_use::mcp::ComputerUseTaskSessionViewV1,
) {
    use crate::computer_use::mcp::ComputerUseTaskSessionStatusV1 as Status;
    match session.status {
        Status::Queued => {
            ui.heading("Queued computer session");
            if let Some(position) = session.queue_position {
                ui.label(format!(
                    "Waiting for VM resources · queue position {position}"
                ));
            } else {
                ui.label("Waiting for available VM resources");
            }
            button(
                ui,
                surface,
                task_id,
                session,
                "Cancel queued request",
                crate::ui::ComputerUseTaskActionV1::StopComputer,
            );
            return;
        }
        Status::Starting => {
            ui.heading("Starting computer session");
            ui.label("The VM is being prepared. Agent actions are not available yet.");
            return;
        }
        Status::Uncertain => {
            ui.heading("Computer session needs attention");
            ui.colored_label(
                crate::ui::theme::WARNING,
                "The last action may have completed. It will not be retried automatically, and control will not return to the agent.",
            );
            if let Some(result) = &session.control_notice {
                ui.label(result);
            }
            ui.add_enabled(false, egui::Button::new("Retry safe step"));
            ui.small("Unavailable: recovery stopped the VM and recorded no retry-safe step.");
            button(
                ui,
                surface,
                task_id,
                session,
                "Inspect uncertain outcome",
                crate::ui::ComputerUseTaskActionV1::InspectUncertainOutcome,
            );
            button(
                ui,
                surface,
                task_id,
                session,
                "Destroy session",
                crate::ui::ComputerUseTaskActionV1::StopComputer,
            );
            return;
        }
        Status::Interrupted => {
            ui.heading("Computer session interrupted");
            ui.colored_label(
                crate::ui::theme::WARNING,
                "This session cannot resume agent control automatically. Review its recorded activity or destroy the session.",
            );
            ui.add_enabled(false, egui::Button::new("Retry safe step"));
            ui.small("Unavailable: recovery stopped the VM and recorded no retry-safe step.");
            button(
                ui,
                surface,
                task_id,
                session,
                "Destroy session",
                crate::ui::ComputerUseTaskActionV1::StopComputer,
            );
            return;
        }
        Status::Active => {}
    }
    let owner = match session.control_owner {
        crate::computer_use::mcp::ComputerUseControlOwnerV1::Agent => "Agent has control",
        crate::computer_use::mcp::ComputerUseControlOwnerV1::Human => "You have control",
        crate::computer_use::mcp::ComputerUseControlOwnerV1::Paused => "Agent paused",
    };
    ui.heading("Live computer");
    let session_class = match session.session_class {
        crate::computer_use::ComputerSessionClassV1::Disposable => "Disposable session",
        crate::computer_use::ComputerSessionClassV1::PersonalPersistent => "Personal session",
    };
    ui.label(format!(
        "{owner} · {} · {session_class}",
        session.agent_role
    ));
    ui.label(format!("{} seconds remain", session.remaining_ms / 1_000));
    ui.label(format!(
        "Budget: {} vCPU · {} MiB RAM · {} actions remaining",
        session.resource_budget.cpu_millis.div_ceil(1_000),
        session.resource_budget.memory_bytes / (1024 * 1024),
        session.actions_remaining,
    ));
    ui.label(format!(
        "Active app: {}",
        session.active_app.as_deref().unwrap_or("Unknown")
    ));
    ui.label(format!("Control generation {}", session.control_generation));
    if session.control_transition_pending {
        ui.colored_label(
            crate::ui::theme::WARNING,
            "Fencing the current action before changing control…",
        );
    }
    if let Some(error) = &session.control_error {
        ui.colored_label(
            crate::ui::theme::WARNING,
            format!("Control change failed: {error}"),
        );
    }

    ui.horizontal_wrapped(|ui| {
        use crate::computer_use::mcp::ComputerUseControlOwnerV1 as Owner;
        match session.control_owner {
            Owner::Agent => {
                button(
                    ui,
                    surface,
                    task_id,
                    session,
                    "Pause Agent",
                    crate::ui::ComputerUseTaskActionV1::PauseAgent,
                );
                button(
                    ui,
                    surface,
                    task_id,
                    session,
                    "Take Control",
                    crate::ui::ComputerUseTaskActionV1::TakeControl,
                );
            }
            Owner::Paused => {
                button(
                    ui,
                    surface,
                    task_id,
                    session,
                    "Resume Agent",
                    crate::ui::ComputerUseTaskActionV1::ResumeAgent,
                );
                button(
                    ui,
                    surface,
                    task_id,
                    session,
                    "Take Control",
                    crate::ui::ComputerUseTaskActionV1::TakeControl,
                );
            }
            Owner::Human => {
                button(
                    ui,
                    surface,
                    task_id,
                    session,
                    "Return Control",
                    crate::ui::ComputerUseTaskActionV1::ReturnControl,
                );
            }
        }
        button(
            ui,
            surface,
            task_id,
            session,
            "Stop Computer",
            crate::ui::ComputerUseTaskActionV1::StopComputer,
        );
    });

    if session.control_owner == crate::computer_use::mcp::ComputerUseControlOwnerV1::Human {
        let mut private = session.private_input;
        let checkbox = ui.checkbox(&mut private, "Private input mode");
        if checkbox.changed() {
            send(
                surface,
                task_id,
                session,
                crate::ui::ComputerUseTaskActionV1::SetPrivateInput(private),
            );
        }
        ui.label(crate::ui::theme::helper_text(
            "When enabled, the viewer blanks the screen and omits keyboard events from its local action journal. The guest app can still read what you type. Use a separately trusted personal computer for account credentials.",
        ));
        if !session.allowed_apps.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for app in &session.allowed_apps {
                    button(
                        ui,
                        surface,
                        task_id,
                        session,
                        &format!("Launch {app}"),
                        crate::ui::ComputerUseTaskActionV1::LaunchApprovedApp(app.clone()),
                    );
                }
            });
        }
    }

    if session.private_input {
        egui::Frame::NONE
            .fill(crate::ui::theme::PANEL_ALT)
            .stroke(egui::Stroke::new(1.0, crate::ui::theme::WARNING))
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Screen hidden during private input").strong());
                ui.label("The VM may still display secrets inside its own applications.");
            });
    } else if let Some(png) = &session.frame_png {
        paint_frame(ui, surface, task_id, session, png);
    } else if session.frame_error {
        ui.colored_label(
            crate::ui::theme::WARNING,
            "Desktop disconnected. Control remains fenced; reconnect or stop the computer.",
        );
    } else {
        ui.label("Connecting to the private desktop stream…");
    }

    ui.collapsing("Recent activity", |ui| {
        if session.action_log.is_empty() {
            ui.label("No actions recorded yet.");
        }
        for entry in session.action_log.iter().rev().take(8) {
            let actor = match entry.actor {
                crate::computer_use::mcp::ComputerUseControlOwnerV1::Agent => "Agent",
                crate::computer_use::mcp::ComputerUseControlOwnerV1::Human => "You",
                crate::computer_use::mcp::ComputerUseControlOwnerV1::Paused => "System",
            };
            ui.label(format!("{actor} · {}", entry.label));
        }
    });
}

mod input;
use input::{button, paint_frame, send};
