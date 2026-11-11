use std::sync::atomic::{AtomicBool, Ordering};

use super::super::super::{ComputerUseCallerV1, ComputerUseConsentGrantV1};
mod activate;
mod guards;
mod personal;
mod session_builder;
use super::super::scheduler::ResourceRequest;
use super::super::session_journal::SessionEventKindV1 as JournalEvent;
use super::super::types::ComputerUseMcpControllerV1;
use super::{APPS, OPS, bounded_budget, unix_time_ms};
use crate::computer_use::{
    ComputerCapabilityV1, ComputerPolicyV1, ComputerSessionClassV1,
    guest_image::{GuestImageOverlayRequestV1, create_guest_image_overlay},
};
use guards::{StartupJournalGuard, StartupViewGuard};
pub(in crate::computer_use::mcp::controller) use personal::{
    PERSONAL_VM_R3_ENABLED, start_personal_session,
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
    controller.cache_queued_view(caller, budget.clone(), lease_expiry);
    let mut startup_view = StartupViewGuard::new(controller, &caller.identity.session_id);
    let lease_id = uuid::Uuid::new_v4().to_string();
    let (allowed_apps, authorized_operations) =
        if caller.role_id == crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_ROLE_V1 {
            (
                request
                    .allowed_apps
                    .iter()
                    .filter(|app| {
                        app.as_str() == crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1
                    })
                    .cloned()
                    .collect(),
                request
                    .allowed_operations
                    .iter()
                    .copied()
                    .filter(|op| {
                        crate::computer_use::mcp::RUNTIME_QA_COMPUTER_USE_OPERATIONS_V1.contains(op)
                    })
                    .collect(),
            )
        } else {
            let mut allowed_apps = request
                .allowed_apps
                .iter()
                .filter(|app| APPS.contains(&app.as_str()))
                .cloned()
                .collect::<std::collections::BTreeSet<_>>();
            if request.network_profile
                == crate::computer_use::ComputerNetworkProfileV1::InternetResearch
            {
                if allowed_apps.remove("chromium") {
                    allowed_apps.insert("chromium-research".into());
                }
            } else {
                allowed_apps.remove("chromium-research");
            }
            let authorized_operations = request
                .allowed_operations
                .iter()
                .copied()
                .filter(|op| OPS.contains(op))
                .collect();
            (allowed_apps, authorized_operations)
        };
    let policy = ComputerPolicyV1 {
        schema_version: 1,
        identity: caller.identity.clone(),
        session_class: ComputerSessionClassV1::Disposable,
        capability: if request.network_profile
            == crate::computer_use::ComputerNetworkProfileV1::InternetResearch
        {
            ComputerCapabilityV1::InternetResearch
        } else {
            ComputerCapabilityV1::ApprovedGuiTask
        },
        network_profile: request.network_profile,
        allowed_apps,
        authorized_operations,
        approval_generation: 1,
        resource_budget: budget.clone(),
    };
    if policy.allowed_apps.is_empty()
        || policy.authorized_operations.is_empty()
        || policy.validate().is_err()
    {
        return Err("invalid_grant_scope");
    }
    let scheduler_permit = runtime
        .scheduler
        .acquire(
            &caller.identity.session_id,
            ResourceRequest {
                vcpus: budget.cpu_millis.div_ceil(1_000) as u16,
                memory_bytes: budget.memory_bytes,
                disk_bytes: budget.disk_bytes,
            },
            cancelled,
        )
        .map_err(|error| match error {
            "cancelled" => "cancelled",
            "computer_session_queue_full" => "session_queue_full",
            "computer_session_queue_expired" => "session_queue_expired",
            "session_resource_budget_exceeds_host_capacity" => "session_resource_limit",
            _ => "session_scheduler_unavailable",
        })?;
    controller.set_start_status(
        &caller.identity.session_id,
        crate::computer_use::mcp::ComputerUseTaskSessionStatusV1::Starting,
    );
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
    let mut session = session_builder::build(session_builder::SessionBuild {
        runtime,
        caller,
        policy,
        network_profile: request.network_profile,
        lease_id,
        expires_at_unix_ms: lease_expiry,
        now_unix_ms: now,
        image: &image,
        session_class: ComputerSessionClassV1::Disposable,
        target: request.target.clone(),
    });
    session
        .attest(runtime.controller_key_id.clone(), &runtime.controller_key)
        .map_err(|_| "attestation_failed")?;
    guards::record_session(runtime, &session, caller, JournalEvent::SessionRequested)?;
    let mut journal_guard = StartupJournalGuard::new(runtime, &session, &caller.role_id);
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
    activate::activate_after_overlay(
        controller,
        runtime,
        caller,
        &session,
        overlay,
        scheduler_permit,
        None,
        initial_control_owner(&caller.role_id),
        false,
        cancelled,
        &mut journal_guard,
        &mut startup_view,
    )
}

fn initial_control_owner(role_id: &str) -> crate::computer_use::mcp::ComputerUseControlOwnerV1 {
    if role_id == crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_ROLE_V1 {
        crate::computer_use::mcp::ComputerUseControlOwnerV1::Human
    } else {
        crate::computer_use::mcp::ComputerUseControlOwnerV1::Agent
    }
}

#[cfg(test)]
#[test]
fn runtime_qa_session_starts_human_owned_while_other_sessions_keep_agent_ownership() {
    assert_eq!(
        initial_control_owner(crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_ROLE_V1),
        crate::computer_use::mcp::ComputerUseControlOwnerV1::Human
    );
    assert_eq!(
        initial_control_owner("implementer/v1"),
        crate::computer_use::mcp::ComputerUseControlOwnerV1::Agent
    );
}
