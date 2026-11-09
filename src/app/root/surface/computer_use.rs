use super::*;

macro_rules! computer_use_surface_methods {
    () => {
        fn computer_use_permissions(&self) -> Vec<crate::ui::ComputerUsePermissionView> {
            self.computer_use_permission_view()
        }

        fn revoke_computer_use_permission(&mut self, session_id: &str) -> Result<(), String> {
            self.revoke_computer_use(session_id)
        }

        fn pending_computer_use_consent_requests(
            &self,
        ) -> Vec<crate::computer_use::mcp::ComputerUseConsentRequestV1> {
            self.pending_computer_use_consent()
        }

        fn decide_computer_use_consent(
            &mut self,
            request_id: &str,
            approve: bool,
        ) -> Result<(), String> {
            self.decide_computer_use_consent_request(request_id, approve)
        }
    };
}

pub(super) use computer_use_surface_methods;

impl KooladeApp {
    pub(super) fn computer_use_permission_view(&self) -> Vec<crate::ui::ComputerUsePermissionView> {
        #[cfg(target_os = "linux")]
        {
            let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
            self.computer_use_authority
                .permissions(now)
                .map(|permissions| {
                    permissions
                        .into_iter()
                        .map(|permission| crate::ui::ComputerUsePermissionView {
                            project_id: permission.identity.project_id,
                            task_id: permission.identity.task_id,
                            user_id: permission.identity.user_id,
                            session_id: permission.identity.session_id,
                            role_id: permission.role_id,
                            machine_id: permission.machine_id,
                            allowed_operations: permission
                                .allowed_operations
                                .into_iter()
                                .map(computer_use_operation_label)
                                .map(str::to_owned)
                                .collect(),
                            network_profile: permission.network_profile,
                            approved_destination: permission.approved_destination,
                            resource_budget: permission.resource_budget,
                            expires_at_unix_ms: permission.expires_at_unix_ms,
                            revoked: permission.revoked,
                            expired: permission.expired,
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        #[cfg(not(target_os = "linux"))]
        {
            Vec::new()
        }
    }

    pub(super) fn revoke_computer_use(&mut self, session_id: &str) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            self.revoke_computer_use_session(session_id)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = session_id;
            Err("Computer Use permission control is unavailable on this host.".into())
        }
    }

    pub(super) fn pending_computer_use_consent(
        &self,
    ) -> Vec<crate::computer_use::mcp::ComputerUseConsentRequestV1> {
        self.computer_use_consent.pending()
    }

    pub(super) fn decide_computer_use_consent_request(
        &mut self,
        request_id: &str,
        approve: bool,
    ) -> Result<(), String> {
        self.computer_use_consent
            .decide(request_id, approve)
            .map_err(str::to_owned)
    }
}

#[cfg(target_os = "linux")]
fn computer_use_operation_label(
    operation: crate::computer_use::ComputerOperationV1,
) -> &'static str {
    use crate::computer_use::ComputerOperationV1 as Operation;
    match operation {
        Operation::ObserveScreen => "observe_screen",
        Operation::ObserveWindows => "observe_windows",
        Operation::ReadAccessibility => "read_accessibility",
        Operation::MouseInput => "mouse_input",
        Operation::KeyboardInput => "keyboard_input",
        Operation::LaunchAllowedApp => "launch_allowed_app",
        Operation::WaitFor => "wait_for",
        Operation::QueryState => "query_state",
    }
}
