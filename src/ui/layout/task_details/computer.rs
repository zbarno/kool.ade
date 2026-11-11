use super::*;

mod personal_profiles;

pub(super) fn paint(ui: &mut egui::Ui, surface: &mut dyn Surface, task_id: &str) {
    personal_profiles::paint(ui, surface);
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
    if let Some(pending) = &session.pending_action_intent {
        use crate::computer_use::ComputerActionIntentStateV1 as IntentState;
        ui.separator();
        ui.heading("Sensitive action approval");
        ui.colored_label(
            crate::ui::theme::WARNING,
            "Review the exact action and current screen before granting this one-time permission.",
        );
        let risk = match pending.intent.risk {
            crate::computer_use::ComputerActionRiskV1::Observation => "Observation",
            crate::computer_use::ComputerActionRiskV1::LocalReversible => "Local reversible change",
            crate::computer_use::ComputerActionRiskV1::ExternalMessage => "External message",
            crate::computer_use::ComputerActionRiskV1::PublicPost => "Public post",
            crate::computer_use::ComputerActionRiskV1::AccountChange => {
                "Account configuration change"
            }
            crate::computer_use::ComputerActionRiskV1::PrivateDataExport => "Private data export",
            crate::computer_use::ComputerActionRiskV1::DestructiveOperation => {
                "Destructive operation"
            }
            crate::computer_use::ComputerActionRiskV1::PaidTransaction => "Paid transaction",
            crate::computer_use::ComputerActionRiskV1::Unclassified => "Unclassified action",
        };
        ui.label(format!("Risk: {risk}"));
        ui.label(format!("Destination: {}", pending.intent.destination));
        ui.label(format!("Affected item: {}", pending.intent.affected_item));
        if let Some(content) = &pending.intent.content_preview {
            ui.label("Exact content:");
            ui.add(egui::Label::new(content).wrap());
        }
        if let (Some(amount), Some(currency)) = (
            pending.intent.amount_minor_units,
            pending.intent.currency.as_deref(),
        ) {
            ui.label(format!("Exact amount: {amount} minor units {currency}"));
        }
        ui.label(format!(
            "Expires at {} · screen revision {}",
            pending.intent.expires_at_unix_ms, pending.intent.frame_revision
        ));
        ui.label(format!("Final GUI action: {:?}", pending.final_action));
        ui.label(format!("Approval state: {:?}", pending.state));
        match pending.state {
            IntentState::AwaitingApproval => {
                ui.horizontal_wrapped(|ui| {
                    button(
                        ui,
                        surface,
                        task_id,
                        session,
                        "Approve once",
                        crate::ui::ComputerUseTaskActionV1::ApproveSensitiveAction(
                            pending.intent.intent_id.clone(),
                        ),
                    );
                    button(
                        ui,
                        surface,
                        task_id,
                        session,
                        "Deny",
                        crate::ui::ComputerUseTaskActionV1::DenySensitiveAction(
                            pending.intent.intent_id.clone(),
                        ),
                    );
                });
            }
            IntentState::Executing => {
                ui.horizontal_wrapped(|ui| {
                    button(
                        ui,
                        surface,
                        task_id,
                        session,
                        "Confirm completed",
                        crate::ui::ComputerUseTaskActionV1::VerifySensitiveActionCompleted(
                            pending.intent.intent_id.clone(),
                        ),
                    );
                    button(
                        ui,
                        surface,
                        task_id,
                        session,
                        "Outcome unknown",
                        crate::ui::ComputerUseTaskActionV1::MarkSensitiveActionOutcomeUnknown(
                            pending.intent.intent_id.clone(),
                        ),
                    );
                });
            }
            IntentState::ApprovedOnce => {
                ui.small("One-time permission is ready for this exact GUI action.");
            }
            IntentState::OutcomeUnknown => {
                ui.colored_label(
                    crate::ui::theme::WARNING,
                    "The external result is uncertain. Do not retry this action.",
                );
            }
            _ => {
                ui.small("This intent is no longer actionable.");
            }
        }
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
        ui.separator();
        ui.heading("Additional applications");
        if session.guest_apps.is_empty() {
            ui.label("No verified guest application catalog is available for this image.");
            ui.small("Package changes stay disabled until a signed catalog and guest helper are promoted.");
        } else {
            for app in &session.guest_apps {
                ui.group(|ui| {
                    ui.label(egui::RichText::new(&app.display_name).strong());
                    ui.label(format!(
                        "{} · {} · {} {} · {:?}",
                        app.publisher, app.source_id, app.package_id, app.version, app.provenance
                    ));
                    ui.small(format!("Package digest: {}", app.package_sha256));
                    ui.small(format!(
                        "Required guest capabilities: {}",
                        app.required_capabilities
                            .iter()
                            .map(|capability| format!("{capability:?}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                    ui.label(if app.installed {
                        "Installed in this disposable guest overlay."
                    } else {
                        "Not installed in this guest."
                    });
                    if !app.session_allowed {
                        ui.small("This app is outside the current session grant.");
                    } else if !session.app_management_available {
                        ui.add_enabled(
                            false,
                            egui::Button::new(if app.installed {
                                "Update / remove unavailable"
                            } else {
                                "Install unavailable"
                            }),
                        );
                    } else if app.installed {
                        ui.horizontal(|ui| {
                            app_operation_button(
                                ui,
                                surface,
                                task_id,
                                session,
                                app,
                                crate::computer_use::apps::GuestAppOperationKindV1::Update,
                                "Update",
                            );
                            app_operation_button(
                                ui,
                                surface,
                                task_id,
                                session,
                                app,
                                crate::computer_use::apps::GuestAppOperationKindV1::Remove,
                                "Remove",
                            );
                        });
                    } else {
                        app_operation_button(
                            ui,
                            surface,
                            task_id,
                            session,
                            app,
                            crate::computer_use::apps::GuestAppOperationKindV1::Install,
                            "Install this exact package",
                        );
                    }
                });
            }
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

fn app_operation_button(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    task_id: &str,
    session: &crate::computer_use::mcp::ComputerUseTaskSessionViewV1,
    app: &crate::computer_use::mcp::ComputerUseGuestAppViewV1,
    operation: crate::computer_use::apps::GuestAppOperationKindV1,
    label: &str,
) {
    let response = ui.button(label);
    if response.clicked() {
        send(
            surface,
            task_id,
            session,
            crate::ui::ComputerUseTaskActionV1::ManageApprovedApp {
                app_id: app.app_id.clone(),
                operation,
            },
        );
    }
    response.on_hover_text(format!(
        "Approved package {} {} · SHA-256 {} · manifest SHA-256 {}",
        app.package_id, app.version, app.package_sha256, app.manifest_sha256
    ));
}

mod input;
use input::{button, paint_frame, send};
