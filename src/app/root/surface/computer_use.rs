use super::*;

macro_rules! computer_use_surface_methods {
    () => {
        fn computer_use_permissions(&self) -> Vec<crate::ui::ComputerUsePermissionView> {
            self.computer_use_permission_view()
        }

        fn computer_use_task_sessions(
            &self,
            task_id: &str,
        ) -> Vec<crate::computer_use::mcp::ComputerUseTaskSessionViewV1> {
            self.computer_use_task_session_view(task_id)
        }

        fn computer_use_personal_profiles(
            &self,
        ) -> Result<Vec<crate::computer_use::mcp::ComputerUsePersonalProfileViewV1>, String> {
            #[cfg(target_os = "linux")]
            {
                self.shared_computer_use_controller()
                    .personal_vm_profile_views_for_current_user()
                    .map_err(|error| format!("Personal VM profiles are unavailable: {error:?}"))
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err("Personal VM profiles are supported only on Linux.".into())
            }
        }

        fn delete_computer_use_personal_profile(&mut self, profile_id: &str) -> Result<(), String> {
            #[cfg(target_os = "linux")]
            {
                self.shared_computer_use_controller()
                    .delete_personal_vm_profile_for_current_user(profile_id)
                    .map_err(|error| format!("Personal VM profile was not deleted: {error:?}"))
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = profile_id;
                Err("Personal VM profiles are supported only on Linux.".into())
            }
        }

        fn lock_computer_use_personal_profile(
            &mut self,
            profile_id: &str,
            session_id: &str,
        ) -> Result<(), String> {
            #[cfg(target_os = "linux")]
            {
                self.shared_computer_use_controller()
                    .lock_personal_vm_session_for_current_user(session_id, profile_id)
                    .map_err(|error| format!("Personal VM was not locked: {error}"))
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (profile_id, session_id);
                Err("Personal VM profiles are supported only on Linux.".into())
            }
        }

        fn computer_use_personal_vm_consent_requests(
            &self,
        ) -> Vec<crate::computer_use::mcp::ComputerUsePersonalVmConsentViewV1> {
            #[cfg(target_os = "linux")]
            {
                self.shared_computer_use_controller()
                    .personal_vm_consent_requests_for_current_user()
            }
            #[cfg(not(target_os = "linux"))]
            {
                Vec::new()
            }
        }

        fn request_computer_use_personal_vm_consent(
            &mut self,
            choice: crate::computer_use::mcp::ComputerUsePersonalVmChoiceV1,
        ) -> Result<(), String> {
            #[cfg(target_os = "linux")]
            {
                self.shared_computer_use_controller()
                    .request_personal_vm_consent_for_current_user(choice)
                    .map(|_| ())
                    .map_err(|error| format!("Personal VM consent request failed: {error}"))
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = choice;
                Err("Personal VM profiles are supported only on Linux.".into())
            }
        }

        fn decide_computer_use_personal_vm_consent(
            &mut self,
            request_id: &str,
            approve: bool,
        ) -> Result<(), String> {
            #[cfg(target_os = "linux")]
            {
                self.shared_computer_use_controller()
                    .decide_personal_vm_consent_for_current_user(request_id, approve)
                    .map_err(|error| format!("Personal VM choice was not applied: {error}"))
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = (request_id, approve);
                Err("Personal VM profiles are supported only on Linux.".into())
            }
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

        fn decide_computer_use_consent_network_profile(
            &mut self,
            request_id: &str,
            profile: crate::computer_use::ComputerNetworkProfileV1,
        ) -> Result<(), String> {
            self.decide_computer_use_consent_network_request(request_id, profile)
        }
    };
}

pub(super) use computer_use_surface_methods;

impl KooladeApp {
    pub(super) fn computer_use_task_session_view(
        &self,
        task_id: &str,
    ) -> Vec<crate::computer_use::mcp::ComputerUseTaskSessionViewV1> {
        #[cfg(target_os = "linux")]
        {
            let uid = unsafe { libc::geteuid() };
            let user_id = uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_OID,
                format!("koolade-linux-effective-uid-v1:{uid}").as_bytes(),
            )
            .to_string();
            self.shared_computer_use_controller()
                .task_session_views(task_id, &user_id)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = task_id;
            Vec::new()
        }
    }

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

    pub(super) fn decide_computer_use_consent_network_request(
        &mut self,
        request_id: &str,
        profile: crate::computer_use::ComputerNetworkProfileV1,
    ) -> Result<(), String> {
        self.computer_use_consent
            .decide_with_network_profile(request_id, profile)
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
        Operation::LaunchApprovedStagedTaskApp => "launch_approved_staged_task_app",
        Operation::ManageApp => "manage_app",
        Operation::WaitFor => "wait_for",
        Operation::QueryState => "query_state",
    }
}
