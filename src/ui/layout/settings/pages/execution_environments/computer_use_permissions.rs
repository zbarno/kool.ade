use crate::ui::{ComputerUsePermissionView, theme};

const PENDING_ID: &str = "execution_environment_computer_use_revoke_pending";
const RESULT_ID: &str = "execution_environment_computer_use_revoke_result";
const CONSENT_RESULT_ID: &str = "execution_environment_computer_use_consent_result";

pub(super) fn paint(
    ui: &mut egui::Ui,
    permissions: &[ComputerUsePermissionView],
    consent_requests: &[crate::computer_use::mcp::ComputerUseConsentRequestV1],
    revoke: &mut impl FnMut(&str) -> Result<(), String>,
    decide: &mut impl FnMut(&str, bool) -> Result<(), String>,
) {
    ui.add_space(theme::spacing::L);
    ui.label(theme::section_heading("Pending Computer Use consent"));
    ui.label(theme::helper_text(
        "A provider request stays blocked until you explicitly approve it. Denying or ignoring it grants no access.",
    ));
    if consent_requests.is_empty() {
        ui.label("No Computer Use requests are waiting for consent.");
    } else {
        for request in consent_requests {
            ui.group(|ui| {
                ui.label(egui::RichText::new("Computer Use access request").strong());
                ui.label(format!("Project ID: {}", request.identity.project_id));
                ui.label(format!("Task ID: {}", request.identity.task_id));
                ui.label(format!("Session ID: {}", request.identity.session_id));
                ui.label(format!("User ID: {}", request.identity.user_id));
                ui.label(format!("Role: {}", request.role_id));
                ui.label(format!("Purpose: {}", request.purpose));
                ui.label(format!("Target VM: {}", request.machine_id));
                ui.label(format!(
                    "Proposed operations: {}",
                    request
                        .allowed_operations
                        .iter()
                        .map(operation_label)
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
                ui.label(format!(
                    "Allowed applications: {}",
                    request.allowed_apps.join(", ")
                ));
                ui.label(format!(
                    "Network access: {}",
                    match request.network_profile {
                        crate::computer_use::ComputerNetworkProfileV1::Disabled => "disabled",
                        crate::computer_use::ComputerNetworkProfileV1::ApprovedInternet => {
                            "approved internet"
                        }
                    }
                ));
                ui.label(format!(
                    "Approved destination: {}",
                    request.approved_destination.as_deref().unwrap_or("none")
                ));
                let budget = &request.resource_budget;
                ui.label(format!(
                    "Resource budget: {} CPU millis, {} MiB memory, {} MiB disk, {} seconds, {} actions",
                    budget.cpu_millis,
                    budget.memory_bytes / (1024 * 1024),
                    budget.disk_bytes / (1024 * 1024),
                    budget.time_limit_ms / 1000,
                    budget.action_limit,
                ));
                ui.horizontal(|ui| {
                    if ui.button("Deny request").clicked() {
                        set_consent_result(
                            ui,
                            decide(&request.request_id, false),
                            "Computer Use request denied.",
                        );
                    }
                    if ui.button("Approve request").clicked() {
                        set_consent_result(
                            ui,
                            decide(&request.request_id, true),
                            "Computer Use request approved.",
                        );
                    }
                });
            });
            ui.add_space(theme::spacing::S);
        }
    }
    if let Some(message) = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(egui::Id::new(CONSENT_RESULT_ID)))
    {
        ui.colored_label(
            if message.starts_with("Computer Use request approved") {
                theme::SUCCESS
            } else {
                theme::WARNING
            },
            message,
        );
    }

    ui.add_space(theme::spacing::L);
    ui.label(theme::section_heading("Active Computer Use permissions"));
    ui.label(theme::helper_text(
        "Review the task, role, virtual machine, allowed actions, and expiry for each app-owned Computer Use grant.",
    ));
    if permissions.is_empty() {
        ui.label("No Computer Use permissions are active.");
        return;
    }

    let mut pending = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(egui::Id::new(PENDING_ID)));
    for permission in permissions {
        ui.group(|ui| {
            ui.label(egui::RichText::new(format!("Task {}", permission.task_id)).strong());
            ui.label(format!("Project: {}", permission.project_id));
            ui.label(format!("Operator: {}", permission.user_id));
            ui.label(format!("Role: {}", permission.role_id));
            ui.label(format!("Session: {}", permission.session_id));
            ui.label(format!("Virtual machine: {}", permission.machine_id));
            ui.label(format!(
                "Allowed actions: {}",
                if permission.allowed_operations.is_empty() {
                    "none".into()
                } else {
                    permission.allowed_operations.join(", ")
                }
            ));
            ui.label(format!(
                "Network access: {}",
                match permission.network_profile {
                    crate::computer_use::ComputerNetworkProfileV1::Disabled => "disabled",
                    crate::computer_use::ComputerNetworkProfileV1::ApprovedInternet => {
                        "approved internet"
                    }
                }
            ));
            ui.label(format!(
                "Approved destination: {}",
                permission.approved_destination.as_deref().unwrap_or("none")
            ));
            ui.label(format!(
                "Resource budget: {} CPU millis, {} MiB memory, {} MiB disk, {} seconds, {} actions",
                permission.resource_budget.cpu_millis,
                permission.resource_budget.memory_bytes / (1024 * 1024),
                permission.resource_budget.disk_bytes / (1024 * 1024),
                permission.resource_budget.time_limit_ms / 1000,
                permission.resource_budget.action_limit,
            ));
            ui.label(format!(
                "Expires: {}",
                expiry_label(permission.expires_at_unix_ms)
            ));
            let state = if permission.revoked {
                "Revoked"
            } else if permission.expired {
                "Expired"
            } else {
                "Active"
            };
            ui.label(format!("Status: {state}"));

            if !permission.revoked && !permission.expired {
                if pending.as_deref() == Some(permission.session_id.as_str()) {
                    ui.label("Revoking immediately blocks future agent calls for this session.");
                    ui.horizontal(|ui| {
                        if ui.button("Confirm revoke").clicked() {
                            let result = revoke(&permission.session_id);
                            let message = match result {
                                Ok(()) => "Computer Use permission revoked.".to_owned(),
                                Err(error) => format!("Could not revoke permission: {error}"),
                            };
                            ui.ctx().data_mut(|data| {
                                data.remove::<String>(egui::Id::new(PENDING_ID));
                                data.insert_temp(egui::Id::new(RESULT_ID), message);
                            });
                            pending = None;
                        }
                        if ui.button("Cancel").clicked() {
                            pending = None;
                        }
                    });
                } else if ui.button("Revoke permission").clicked() {
                    pending = Some(permission.session_id.clone());
                }
            }
        });
        ui.add_space(theme::spacing::S);
    }
    ui.ctx().data_mut(|data| match pending {
        Some(session_id) => {
            data.insert_temp(egui::Id::new(PENDING_ID), session_id);
        }
        None => {
            data.remove::<String>(egui::Id::new(PENDING_ID));
        }
    });
    if let Some(message) = ui
        .ctx()
        .data_mut(|data| data.get_temp::<String>(egui::Id::new(RESULT_ID)))
    {
        ui.colored_label(
            if message.starts_with("Computer Use permission revoked") {
                theme::SUCCESS
            } else {
                theme::WARNING
            },
            message,
        );
    }
}

fn set_consent_result(ui: &mut egui::Ui, result: Result<(), String>, success: &'static str) {
    let message = result.map_or_else(
        |error| format!("Could not record consent decision: {error}"),
        |_| success.to_owned(),
    );
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new(CONSENT_RESULT_ID), message));
}

fn expiry_label(unix_ms: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(unix_ms.min(i64::MAX as u64) as i64)
        .map(|timestamp| timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn operation_label(operation: &crate::computer_use::ComputerOperationV1) -> &'static str {
    use crate::computer_use::ComputerOperationV1 as Operation;
    match operation {
        Operation::ObserveScreen => "observe screen",
        Operation::ObserveWindows => "observe windows",
        Operation::ReadAccessibility => "read accessibility",
        Operation::MouseInput => "mouse input",
        Operation::KeyboardInput => "keyboard input",
        Operation::LaunchAllowedApp => "launch allowed app",
        Operation::WaitFor => "wait for condition",
        Operation::QueryState => "query state",
    }
}

#[cfg(test)]
#[path = "computer_use_permissions_tests.rs"]
mod tests;
