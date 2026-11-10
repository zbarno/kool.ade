use std::sync::atomic::AtomicBool;

use crate::computer_use::{
    ComputerActionActorV1 as Actor, ComputerOperationV1 as Operation,
    apps::{
        GuestAppCapabilityV1 as Capability, GuestAppConsentGrantV1,
        GuestAppOperationKindV1 as Kind, GuestAppOperationRequestV1, GuestAppProvisionerV1,
    },
    mcp::{
        ComputerUseCallerV1 as Caller, ComputerUseControlOwnerV1 as Owner,
        ComputerUseMcpRequestV1 as Request, ComputerUseMcpStatusV1 as Status,
    },
};

use super::super::{session::unix_time_ms, types::ComputerUseMcpControllerV1};

impl ComputerUseMcpControllerV1 {
    /// Apply a catalog-bound package operation from the explicit human control
    /// surface. Existing session and guest action journals own replay/fencing.
    pub fn manage_approved_app(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        app_id: &str,
        operation: Kind,
    ) -> Result<(), &'static str> {
        let (caller, action) = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| "session_state_unavailable")?;
            let active = sessions.get(session_id).ok_or("session_not_active")?;
            if active.session.identity.task_id != task_id
                || active.session.identity.user_id != user_id
                || active.control_owner != Owner::Human
                || active.uncertain
                || active.paused
                || active.session.session_class
                    != crate::computer_use::ComputerSessionClassV1::Disposable
                || !active.session.allowed_apps.contains(app_id)
                || !active
                    .session
                    .policy
                    .authorized_operations
                    .contains(&Operation::ManageApp)
            {
                return Err("approved_app_operation_not_available");
            }
            let runtime = self.runtime.as_ref().ok_or("trusted_runtime_unavailable")?;
            let catalog = runtime
                .application_catalog
                .as_ref()
                .ok_or("signed_app_catalog_unavailable")?;
            let manifest = catalog
                .app(app_id)
                .ok_or("app_is_not_in_the_signed_catalog")?;
            if !active
                .channel
                .as_ref()
                .is_some_and(|channel| channel.capabilities().app_management)
            {
                return Err("guest_package_helper_unavailable");
            }
            if !manifest
                .supported_image_digests
                .contains(&active.session.image_digest)
                || !manifest
                    .supported_architectures
                    .contains(&crate::computer_use::apps::GuestAppArchitectureV1::X86_64)
                || manifest
                    .required_capabilities
                    .iter()
                    .any(|capability| *capability != Capability::GraphicalDisplay)
            {
                return Err("app_capabilities_exceed_session_grant");
            }
            let installed = active.channel.as_ref().is_some_and(|channel| {
                channel
                    .capabilities()
                    .app_registry
                    .iter()
                    .any(|installed| installed == app_id)
            });
            match operation {
                Kind::Install if installed => return Err("app_already_installed_use_update"),
                Kind::Update if !installed => return Err("app_not_installed_cannot_update"),
                Kind::Remove if !installed => return Err("app_not_installed_cannot_remove"),
                Kind::Launch => return Err("use_approved_app_launch"),
                _ => {}
            }

            let now = unix_time_ms();
            let granted_capabilities = manifest.required_capabilities.clone();
            let request = GuestAppOperationRequestV1 {
                schema_version: 1,
                operation_id: uuid::Uuid::new_v4().to_string(),
                identity: active.session.identity.clone(),
                session_class: active.session.session_class,
                app_id: manifest.app_id.clone(),
                operation,
                catalog_digest: catalog.digest().to_owned(),
                image_digest: active.session.image_digest.clone(),
                architecture: catalog.catalog().architecture,
                lease_generation: active.session.lease.generation,
                approval_generation: active.session.approval_generation,
                now_unix_ms: now,
                granted_capabilities: granted_capabilities.clone(),
                network_granted: false,
            };
            let approval = GuestAppConsentGrantV1::approved_by_controller(
                request.identity.clone(),
                request.session_class,
                request.app_id.clone(),
                request.operation,
                request.catalog_digest.clone(),
                request.image_digest.clone(),
                request.architecture,
                request.lease_generation,
                request.approval_generation,
                now.saturating_add(60_000),
                granted_capabilities,
                false,
            );
            let action = GuestAppProvisionerV1::new(catalog.clone()).computer_action(
                &request,
                &approval,
                active.next_action_sequence,
                active.session.cancellation_generation,
            )?;
            let caller = Caller {
                identity: active.session.identity.clone(),
                actor: Actor::Human,
                role_id: active.role_id.clone(),
            };
            (caller, action)
        };

        let authority = self
            .registry
            .authorize(
                &caller.identity,
                Actor::Human,
                &caller.role_id,
                unix_time_ms(),
            )
            .map_err(|_| "human_control_not_authorized")?;
        let response = super::super::dispatch::dispatch(
            self,
            &caller,
            authority,
            Request::Action {
                session_id: session_id.to_owned(),
                payload: action.payload,
                timeout_ms: 60_000,
                idempotency_key: Some(action.idempotency_key),
            },
            &AtomicBool::new(false),
        );
        if response.status == Status::Completed {
            Ok(())
        } else {
            Err("approved_app_operation_failed")
        }
    }
}
