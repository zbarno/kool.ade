use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

mod response;
mod startup;
pub(crate) use response::unix_time_ms;
pub(super) use response::{completed, denied, failed, response};

use crate::computer_use::{
    ComputerNetworkProfileV1, ComputerOperationV1, ComputerResourceBudgetV1, ComputerSessionV1,
};

use super::super::{
    ComputerUseCallerV1, ComputerUseConsentGrantV1, ComputerUseConsentScopeV1,
    ComputerUseMcpOperationsV1, ComputerUseMcpRequestV1, ComputerUseMcpResponseV1,
    ComputerUseMcpStatusV1, is_supported_computer_use_role_v1,
};
use super::types::ComputerUseMcpControllerV1;

const OPS: [ComputerOperationV1; 7] = [
    ComputerOperationV1::ObserveScreen,
    ComputerOperationV1::ObserveWindows,
    ComputerOperationV1::MouseInput,
    ComputerOperationV1::KeyboardInput,
    ComputerOperationV1::LaunchAllowedApp,
    ComputerOperationV1::WaitFor,
    ComputerOperationV1::QueryState,
];
const APPS: [&str; 3] = ["chromium", "file-manager", "text-editor"];

impl ComputerUseMcpOperationsV1 for ComputerUseMcpControllerV1 {
    fn capabilities(&self, caller: &ComputerUseCallerV1) -> ComputerUseMcpResponseV1 {
        if let Err(response) = self.consent_scope(caller) {
            return *response;
        }
        response(
            ComputerUseMcpStatusV1::Completed,
            "Computer Use runtime is ready.",
            None,
            None,
            vec![
                "computer_use.capabilities".into(),
                "computer_use.request_session".into(),
                "computer_use.start_session".into(),
                "computer_use.observe".into(),
                "computer_use.action".into(),
                "computer_use.pause".into(),
                "computer_use.resume".into(),
                "computer_use.release".into(),
            ],
        )
    }

    fn consent_scope(
        &self,
        caller: &ComputerUseCallerV1,
    ) -> Result<ComputerUseConsentScopeV1, Box<ComputerUseMcpResponseV1>> {
        if caller.identity.validate().is_err()
            || !is_supported_computer_use_role_v1(&caller.role_id)
        {
            return Err(Box::new(denied("authorization_failed")));
        }
        let Some(runtime) = self.runtime.as_ref() else {
            return Err(Box::new(
                self.blocked(
                    self.unavailable_reason
                        .as_deref()
                        .unwrap_or("trusted_runtime_unavailable"),
                ),
            ));
        };
        if runtime.backend.probe().map_or(true, |probe| {
            !probe.linux_host
                || !probe.kvm_accessible
                || !probe.qemu_available
                || !probe.delegated_resource_controls
        }) || runtime
            .cache
            .verify_cached_catalog_image(
                &runtime.manifest_bytes,
                &runtime.trust,
                &runtime.generation_floor,
                crate::computer_use::guest_image::ComputerGuestArchitectureV1::X86_64,
            )
            .is_err()
        {
            return Err(Box::new(
                self.blocked("trusted_runtime_or_image_unavailable"),
            ));
        }
        Ok(ComputerUseConsentScopeV1 {
            machine_id: runtime.machine_id.clone(),
            approved_destination: None,
            allowed_operations: OPS.to_vec(),
            allowed_apps: APPS.iter().map(|app| (*app).to_owned()).collect(),
            network_profile: ComputerNetworkProfileV1::Disabled,
            resource_budget: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
        })
    }

    fn request_session(
        &self,
        caller: &ComputerUseCallerV1,
        grant: &ComputerUseConsentGrantV1,
        cancelled: &AtomicBool,
    ) -> ComputerUseMcpResponseV1 {
        if cancelled.load(Ordering::Acquire) {
            return denied("cancelled");
        }
        let Some(runtime) = self.runtime.as_ref() else {
            return self.blocked(
                self.unavailable_reason
                    .as_deref()
                    .unwrap_or("trusted_runtime_unavailable"),
            );
        };
        let request = &grant.request;
        if cancelled.load(Ordering::Acquire) {
            return denied("cancelled");
        }
        if request.identity != caller.identity
            || request.role_id != caller.role_id
            || request.machine_id != runtime.machine_id
            || request.approved_destination.is_some()
            || request.network_profile != ComputerNetworkProfileV1::Disabled
            || grant.expires_at_unix_ms <= unix_time_ms()
        {
            return denied("consent_grant_mismatch");
        }
        let Ok(false) = self.registry.contains_session(&caller.identity.session_id) else {
            return denied("session_already_registered");
        };
        if let Err(error) = startup::start_granted_session(self, runtime, caller, grant, cancelled)
        {
            let code = if error == "cancelled" {
                "cancelled"
            } else {
                "computer_use_start_failed"
            };
            return response(
                if error == "cancelled" {
                    ComputerUseMcpStatusV1::Denied
                } else {
                    ComputerUseMcpStatusV1::Failed
                },
                "Computer Use session could not be started safely.",
                Some(code),
                None,
                Vec::new(),
            );
        }
        response(
            ComputerUseMcpStatusV1::Ready,
            "Approved Computer Use session is ready.",
            None,
            Some(caller.identity.session_id.clone()),
            Vec::new(),
        )
    }

    fn dispatch_authorized(
        &self,
        caller: &ComputerUseCallerV1,
        authority: Arc<crate::computer_use::guest_protocol::AuthorizedComputerUseSessionV1>,
        request: ComputerUseMcpRequestV1,
        cancelled: &AtomicBool,
    ) -> ComputerUseMcpResponseV1 {
        super::dispatch::dispatch(self, caller, authority, request, cancelled)
    }

    fn revoke_session(
        &self,
        caller: &ComputerUseCallerV1,
        session_id: &str,
    ) -> ComputerUseMcpResponseV1 {
        ComputerUseMcpControllerV1::revoke_session(self, caller, session_id)
    }
}

impl ComputerUseMcpControllerV1 {
    pub fn revoke_session(
        &self,
        caller: &ComputerUseCallerV1,
        session_id: &str,
    ) -> ComputerUseMcpResponseV1 {
        if caller.identity.validate().is_err()
            || caller.identity.session_id != session_id
            || !is_supported_computer_use_role_v1(&caller.role_id)
        {
            return denied("authorization_failed");
        }
        let active_identity = self.sessions.lock().ok().and_then(|sessions| {
            sessions
                .get(session_id)
                .map(|active| active.session.identity.clone())
        });
        if active_identity
            .as_ref()
            .is_some_and(|identity| identity != &caller.identity)
        {
            return denied("session_not_active");
        }
        if active_identity.is_none() {
            return match self.registry.contains_session(session_id) {
                Ok(false) => completed(session_id),
                Ok(true) => {
                    self.registry.revoke(session_id).ok();
                    failed("session_cleanup_state_missing")
                }
                Err(_) => failed("authorization_unavailable"),
            };
        }
        match self.revoke(session_id) {
            Ok(()) => completed(session_id),
            Err(reason) => failed(reason),
        }
    }

    pub fn revoke(&self, session_id: &str) -> Result<(), &'static str> {
        let runtime = self.runtime.as_ref().ok_or("trusted_runtime_unavailable")?;
        let revoked = self.registry.revoke(session_id);
        let active = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?
            .remove(session_id);
        if let Some(active) = active {
            runtime
                .backend
                .release(&active.session, &active.handle, unix_time_ms())
                .map_err(|_| "backend_release_failed")?;
        }
        revoked.map_err(|_| "authority_revocation_failed")?;
        Ok(())
    }
}

fn cleanup(
    runtime: &super::types::Runtime,
    session: &ComputerSessionV1,
    handle: &crate::computer_use::ComputerMachineHandleV1,
) {
    runtime.backend.cancel(session, handle, unix_time_ms()).ok();
    runtime
        .backend
        .release(session, handle, unix_time_ms())
        .ok();
}

pub(super) fn bounded_budget(requested: &ComputerResourceBudgetV1) -> ComputerResourceBudgetV1 {
    let ceiling = crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1;
    ComputerResourceBudgetV1 {
        cpu_millis: requested.cpu_millis.min(ceiling.cpu_millis),
        memory_bytes: requested.memory_bytes.min(ceiling.memory_bytes),
        disk_bytes: requested.disk_bytes.min(ceiling.disk_bytes),
        time_limit_ms: requested.time_limit_ms.min(ceiling.time_limit_ms),
        action_limit: requested.action_limit.min(ceiling.action_limit),
    }
}
