use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use super::super::super::{ComputerUseCallerV1, ComputerUseConsentGrantV1};
use super::super::types::{ActiveSession, ComputerUseMcpControllerV1};
use super::{APPS, OPS, bounded_budget, cleanup, unix_time_ms};
use crate::computer_use::{
    ComputerCapabilityV1, ComputerLifecycleV1, ComputerPolicyV1, ComputerSessionClassV1,
    ComputerSessionV1, ComputerTargetV1,
    guest_image::{GuestImageOverlayRequestV1, create_guest_image_overlay},
};

pub(super) fn start_granted_session(
    controller: &ComputerUseMcpControllerV1,
    runtime: &super::super::types::Runtime,
    caller: &ComputerUseCallerV1,
    grant: &ComputerUseConsentGrantV1,
    cancelled: &AtomicBool,
) -> Result<(), &'static str> {
    let now = unix_time_ms();
    let request = &grant.request;
    let budget = bounded_budget(&request.resource_budget);
    let lease_expiry = grant
        .expires_at_unix_ms
        .min(now.saturating_add(budget.time_limit_ms))
        .min(now.saturating_add(15 * 60 * 1_000));
    if lease_expiry <= now {
        return Err("invalid_grant");
    }
    let lease_id = uuid::Uuid::new_v4().to_string();
    let policy = ComputerPolicyV1 {
        schema_version: 1,
        identity: caller.identity.clone(),
        session_class: ComputerSessionClassV1::Disposable,
        capability: ComputerCapabilityV1::ApprovedGuiTask,
        network_profile: request.network_profile,
        allowed_apps: request
            .allowed_apps
            .iter()
            .filter(|app| APPS.contains(&app.as_str()))
            .cloned()
            .collect(),
        authorized_operations: request
            .allowed_operations
            .iter()
            .copied()
            .filter(|op| OPS.contains(op))
            .collect(),
        approval_generation: 1,
        resource_budget: budget,
    };
    if policy.allowed_apps.is_empty()
        || policy.authorized_operations.is_empty()
        || policy.validate().is_err()
    {
        return Err("invalid_grant_scope");
    }
    let image = runtime
        .cache
        .verify_cached_catalog_image(
            &runtime.manifest_bytes,
            &runtime.trust,
            &runtime.generation_floor,
            crate::computer_use::guest_image::ComputerGuestArchitectureV1::X86_64,
        )
        .map_err(|_| "trusted_image_unavailable")?;
    if image.entry().image_digest != runtime.image.entry().image_digest
        || image.manifest_digest() != runtime.image.manifest_digest()
    {
        return Err("trusted_image_changed");
    }
    let mut session = ComputerSessionV1 {
        schema_version: 1,
        session_started_at_unix_ms: now,
        identity: caller.identity.clone(),
        session_class: ComputerSessionClassV1::Disposable,
        target: ComputerTargetV1::LocalKvm {
            machine_id: runtime.machine_id.clone(),
        },
        image_digest: image.entry().image_digest.clone(),
        image_manifest_digest: image.manifest_digest().to_owned(),
        image_signer_key_id: image.manifest_signer_key_id().to_owned(),
        network_profile: request.network_profile,
        allowed_apps: policy.allowed_apps.clone(),
        policy,
        lease: crate::computer_use::ComputerLeaseV1 {
            schema_version: 1,
            owner_id: caller.identity.user_id.clone(),
            lease_id,
            generation: 1,
            issued_at_unix_ms: now,
            expires_at_unix_ms: lease_expiry,
            cancellation_generation: 1,
        },
        lifecycle: ComputerLifecycleV1::Delegated,
        action_sequence: 0,
        approval_generation: 1,
        cancellation_generation: 1,
        evidence_refs: Vec::new(),
        controller_proof: crate::computer_use::ComputerControllerProofV1 {
            signer_key_id: runtime.controller_key_id.clone(),
            signature_hex: "0".repeat(128),
        },
    };
    session
        .attest(runtime.controller_key_id.clone(), &runtime.controller_key)
        .map_err(|_| "attestation_failed")?;
    if cancelled.load(Ordering::Acquire) {
        return Err("cancelled");
    }
    let overlay = create_guest_image_overlay(
        &runtime.cache,
        GuestImageOverlayRequestV1 {
            manifest_bytes: &runtime.manifest_bytes,
            trust: &runtime.trust,
            session: &session,
            expected_architecture:
                crate::computer_use::guest_image::ComputerGuestArchitectureV1::X86_64,
            controller_key_id: &runtime.controller_key_id,
            controller_key: &runtime.controller_key.verifying_key(),
            generation_floor: &runtime.generation_floor,
            overlay_root: &runtime.overlay_root,
            qemu_tool: &runtime.qemu_tool,
        },
    )
    .map_err(|_| "overlay_unavailable")?;
    if cancelled.load(Ordering::Acquire) {
        overlay.release_after_vm_stopped().ok();
        return Err("cancelled");
    }
    let (handle, _) = runtime
        .backend
        .prepare(&session, overlay, unix_time_ms())
        .map_err(|_| "backend_prepare_failed")?;
    if cancelled.load(Ordering::Acquire) {
        cleanup(runtime, &session, &handle);
        return Err("cancelled");
    }
    if runtime
        .backend
        .boot(&session, &handle, unix_time_ms())
        .is_err()
    {
        cleanup(runtime, &session, &handle);
        return Err("backend_boot_failed");
    }
    if cancelled.load(Ordering::Acquire) {
        cleanup(runtime, &session, &handle);
        return Err("cancelled");
    }
    #[cfg(target_os = "linux")]
    let channel = match runtime
        .backend
        .authenticate_guest(&session, &handle, unix_time_ms())
    {
        Ok(channel) => channel,
        Err(_) => {
            cleanup(runtime, &session, &handle);
            return Err("guest_authentication_failed");
        }
    };
    #[cfg(not(target_os = "linux"))]
    {
        cleanup(runtime, &session, &handle);
        return Err("unsupported_host");
    }
    if cancelled.load(Ordering::Acquire) {
        cleanup(runtime, &session, &handle);
        return Err("cancelled");
    }
    if runtime
        .backend
        .wait_ready(&session, &handle, Duration::from_secs(120), unix_time_ms())
        .is_err()
    {
        cleanup(runtime, &session, &handle);
        return Err("guest_not_ready");
    }
    if cancelled.load(Ordering::Acquire) {
        cleanup(runtime, &session, &handle);
        return Err("cancelled");
    }
    let journal = match runtime
        .backend
        .open_guest_action_journal(&session, &handle, unix_time_ms())
    {
        Ok(journal) => journal,
        Err(_) => {
            cleanup(runtime, &session, &handle);
            return Err("journal_unavailable");
        }
    };
    let mut sessions = match controller.sessions.lock() {
        Ok(sessions) => sessions,
        Err(_) => {
            cleanup(runtime, &session, &handle);
            return Err("session_state_unavailable");
        }
    };
    if sessions.contains_key(&caller.identity.session_id) {
        cleanup(runtime, &session, &handle);
        return Err("session_already_active");
    }
    if controller
        .registry
        .register(
            session.clone(),
            runtime.controller_key_id.clone(),
            runtime.controller_key.verifying_key(),
            caller.actor,
            caller.role_id.clone(),
            unix_time_ms(),
        )
        .is_err()
    {
        cleanup(runtime, &session, &handle);
        return Err("authority_registration_failed");
    }
    if cancelled.load(Ordering::Acquire) {
        controller.registry.revoke(&caller.identity.session_id).ok();
        cleanup(runtime, &session, &handle);
        return Err("cancelled");
    }
    sessions.insert(
        caller.identity.session_id.clone(),
        ActiveSession {
            session,
            handle,
            journal,
            channel: Some(channel),
            started: true,
            paused: false,
            uncertain: false,
            next_action_sequence: 1,
            role_id: caller.role_id.clone(),
            control_owner: crate::computer_use::mcp::ComputerUseControlOwnerV1::Agent,
            control_generation: 1,
            require_fresh_agent_observation: false,
            private_input: false,
            active_app: None,
            action_log: std::collections::VecDeque::new(),
            display_io: std::sync::Arc::new(std::sync::Mutex::new(())),
        },
    );
    if let Some(active) = sessions.get(&caller.identity.session_id) {
        controller.cache_active_view(active);
    }
    Ok(())
}
