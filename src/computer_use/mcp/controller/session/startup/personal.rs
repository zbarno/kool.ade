use std::sync::atomic::{AtomicBool, Ordering};

use crate::computer_use::{
    ComputerActionActorV1, ComputerCapabilityV1, ComputerNetworkProfileV1, ComputerOperationV1,
    ComputerPolicyV1, ComputerResourceBudgetV1, ComputerSessionClassV1,
    guest_image::ComputerGuestArchitectureV1,
    mcp::{
        ComputerUseControlOwnerV1, ComputerUsePersonalVmChoiceV1,
        ComputerUsePersonalVmSessionTargetV1,
    },
};

use super::super::super::session_journal::SessionEventKindV1 as JournalEvent;
use super::super::super::types::{ComputerUseMcpControllerV1, Runtime};
use super::super::{bounded_budget, unix_time_ms};
use super::{activate, guards, session_builder};
use crate::computer_use::mcp::ComputerUseCallerV1;

#[cfg(test)]
mod tests;

const PERSONAL_ROLE: &str = "implementer/v1";
pub(in crate::computer_use::mcp::controller) const PERSONAL_VM_R3_ENABLED: bool = false;
const PERSONAL_APPS: [&str; 3] = ["chromium", "file-manager", "text-editor"];
const PERSONAL_OPERATIONS: [ComputerOperationV1; 8] = [
    ComputerOperationV1::ObserveScreen,
    ComputerOperationV1::ObserveWindows,
    ComputerOperationV1::ReadAccessibility,
    ComputerOperationV1::MouseInput,
    ComputerOperationV1::KeyboardInput,
    ComputerOperationV1::LaunchAllowedApp,
    ComputerOperationV1::WaitFor,
    ComputerOperationV1::QueryState,
];
const PERSONAL_SESSION_MAX_MS: u64 = 24 * 60 * 60 * 1_000;

/// Starts a profile session only after the controller-owned local consent path
/// has selected an exact profile and session target. The production release
/// gate remains false; tests cover the enabled authorization path.
pub(in crate::computer_use::mcp::controller) fn start_personal_session(
    controller: &ComputerUseMcpControllerV1,
    runtime: &Runtime,
    choice: &ComputerUsePersonalVmChoiceV1,
    target: &ComputerUsePersonalVmSessionTargetV1,
    owner_id: &str,
    cancelled: &AtomicBool,
) -> Result<(), &'static str> {
    start_personal_session_with_gate(
        controller,
        runtime,
        choice,
        target,
        owner_id,
        cancelled,
        PERSONAL_VM_R3_ENABLED,
    )
}

fn start_personal_session_with_gate(
    controller: &ComputerUseMcpControllerV1,
    runtime: &Runtime,
    choice: &ComputerUsePersonalVmChoiceV1,
    target: &ComputerUsePersonalVmSessionTargetV1,
    owner_id: &str,
    cancelled: &AtomicBool,
    release_enabled: bool,
) -> Result<(), &'static str> {
    authorize_personal_start(choice, target, owner_id, release_enabled)?;
    if cancelled.load(Ordering::Acquire) {
        return Err("cancelled");
    }
    let now = unix_time_ms();
    let image = runtime
        .cache
        .verify_cached_catalog_image(
            &runtime.manifest_bytes,
            &runtime.trust,
            &runtime.generation_floor,
            ComputerGuestArchitectureV1::X86_64,
        )
        .map_err(|_| "trusted_image_unavailable")?;
    if image.entry().image_digest != runtime.image.entry().image_digest
        || image.manifest_digest() != runtime.image.manifest_digest()
    {
        return Err("trusted_image_changed");
    }
    let store = runtime
        .personal_profiles
        .as_ref()
        .ok_or("personal_vm_key_service_unavailable")?;
    match choice {
        ComputerUsePersonalVmChoiceV1::Create { retention_days } => {
            if !(1..=365).contains(retention_days) {
                return Err("personal_vm_retention_out_of_range");
            }
            let retention = now.saturating_add(u64::from(*retention_days) * 24 * 60 * 60 * 1_000);
            let (_, overlay) = store
                .create_encrypted_session_disk(
                    owner_id,
                    crate::computer_use::personal_storage::PersonalVmCreateRequestV1 {
                        profile_id: &target.profile_id,
                        session_id: &target.identity.session_id,
                        now_unix_ms: now,
                        retention_until_unix_ms: retention,
                        image: &image,
                        qemu_tool: &runtime.qemu_tool,
                    },
                )
                .map_err(|_| "personal_vm_profile_create_failed")?;
            activate_personal_overlay(controller, runtime, target, now, overlay, cancelled)
        }
        ComputerUsePersonalVmChoiceV1::Unlock { .. } => {
            let overlay = store
                .unlock_profile_for_session(
                    owner_id,
                    &target.profile_id,
                    &target.identity.session_id,
                    now,
                    &image,
                )
                .map_err(|_| "personal_vm_profile_unlock_failed")?;
            activate_personal_overlay(controller, runtime, target, now, overlay, cancelled)
        }
    }
}

fn authorize_personal_start(
    choice: &ComputerUsePersonalVmChoiceV1,
    target: &ComputerUsePersonalVmSessionTargetV1,
    owner_id: &str,
    release_enabled: bool,
) -> Result<(), &'static str> {
    if !release_enabled {
        return Err("personal_vm_r3_security_gate_disabled");
    }
    validate_target(choice, target, owner_id)
}

fn activate_personal_overlay(
    controller: &ComputerUseMcpControllerV1,
    runtime: &Runtime,
    target: &ComputerUsePersonalVmSessionTargetV1,
    now: u64,
    overlay: crate::computer_use::guest_image::GuestImageOverlayV1,
    cancelled: &AtomicBool,
) -> Result<(), &'static str> {
    if !target.private_input_required
        || target.initial_control_owner != ComputerUseControlOwnerV1::Human
    {
        return Err("personal_vm_human_private_input_required");
    }
    let budget = ComputerResourceBudgetV1 {
        cpu_millis: 2_000,
        memory_bytes: 1_073_741_824,
        disk_bytes: 2_147_483_648,
        time_limit_ms: PERSONAL_SESSION_MAX_MS,
        action_limit: 10_000,
    };
    let budget = bounded_budget(&budget);
    let lease_expiry = now
        .saturating_add(budget.time_limit_ms)
        .min(now.saturating_add(PERSONAL_SESSION_MAX_MS));
    let caller = ComputerUseCallerV1 {
        identity: target.identity.clone(),
        actor: ComputerActionActorV1::Human,
        role_id: PERSONAL_ROLE.to_owned(),
    };
    controller.cache_queued_view(&caller, budget.clone(), lease_expiry);
    let mut startup_view = guards::StartupViewGuard::new(controller, &caller.identity.session_id);
    let permit = runtime
        .scheduler
        .acquire(
            &caller.identity.session_id,
            super::super::super::scheduler::ResourceRequest {
                vcpus: budget.cpu_millis.div_ceil(1_000) as u16,
                memory_bytes: budget.memory_bytes,
                disk_bytes: budget.disk_bytes,
            },
            cancelled,
        )
        .map_err(|_| "personal_vm_scheduler_unavailable")?;
    controller.set_start_status(
        &caller.identity.session_id,
        crate::computer_use::mcp::ComputerUseTaskSessionStatusV1::Starting,
    );
    let policy = ComputerPolicyV1 {
        schema_version: 1,
        identity: caller.identity.clone(),
        session_class: ComputerSessionClassV1::PersonalPersistent,
        capability: ComputerCapabilityV1::PersistentPersonal,
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps: PERSONAL_APPS.iter().map(|app| (*app).to_owned()).collect(),
        authorized_operations: PERSONAL_OPERATIONS.into_iter().collect(),
        approval_generation: 1,
        resource_budget: budget.clone(),
    };
    policy
        .validate()
        .map_err(|_| "personal_vm_policy_invalid")?;
    let mut session = session_builder::build(session_builder::SessionBuild {
        runtime,
        caller: &caller,
        policy,
        network_profile: ComputerNetworkProfileV1::Disabled,
        lease_id: uuid::Uuid::new_v4().to_string(),
        expires_at_unix_ms: lease_expiry,
        now_unix_ms: now,
        image: &runtime.image,
        session_class: ComputerSessionClassV1::PersonalPersistent,
    });
    session
        .attest(runtime.controller_key_id.clone(), &runtime.controller_key)
        .map_err(|_| "attestation_failed")?;
    guards::record_session(runtime, &session, &caller, JournalEvent::SessionRequested)?;
    let mut journal_guard = guards::StartupJournalGuard::new(runtime, &session, &caller.role_id);
    if cancelled.load(Ordering::Acquire) {
        return Err("cancelled");
    }
    activate::activate_after_overlay(
        controller,
        runtime,
        &caller,
        &session,
        overlay,
        permit,
        Some(target.profile_id.clone()),
        ComputerUseControlOwnerV1::Human,
        true,
        cancelled,
        &mut journal_guard,
        &mut startup_view,
    )
}

fn validate_target(
    choice: &ComputerUsePersonalVmChoiceV1,
    target: &ComputerUsePersonalVmSessionTargetV1,
    owner_id: &str,
) -> Result<(), &'static str> {
    let expected_profile = match choice {
        ComputerUsePersonalVmChoiceV1::Create { retention_days }
            if (1..=365).contains(retention_days) =>
        {
            &target.profile_id
        }
        ComputerUsePersonalVmChoiceV1::Create { .. } => {
            return Err("personal_vm_retention_out_of_range");
        }
        ComputerUsePersonalVmChoiceV1::Unlock { profile_id } => profile_id,
    };
    let expected_project = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_OID,
        format!("koolade-personal-vm-project-v1:{owner_id}").as_bytes(),
    )
    .to_string();
    if target.identity.validate().is_err()
        || target.identity.user_id != owner_id
        || target.identity.project_id != expected_project
        || target.identity.task_id != target.profile_id
        || target.profile_id.as_str() != expected_profile.as_str()
        || target.session_class != ComputerSessionClassV1::PersonalPersistent
        || target.network_profile != ComputerNetworkProfileV1::Disabled
        || target.initial_control_owner != ComputerUseControlOwnerV1::Human
        || !target.private_input_required
    {
        return Err("personal_vm_target_mismatch");
    }
    Ok(())
}
