use std::sync::atomic::{AtomicBool, Ordering};

use crate::computer_use::mcp::ComputerUseCallerV1;
use crate::computer_use::{ComputerSessionV1, guest_image::GuestImageOverlayV1};

use super::super::super::{
    scheduler::SessionPermit,
    session_journal::SessionEventKindV1 as JournalEvent,
    types::{ActiveSession, ComputerUseMcpControllerV1, Runtime},
    unix_time_ms,
};
use super::guards::{StartupJournalGuard, StartupViewGuard};

#[allow(clippy::too_many_arguments)]
pub(super) fn activate_after_overlay(
    controller: &ComputerUseMcpControllerV1,
    runtime: &Runtime,
    caller: &ComputerUseCallerV1,
    session: &ComputerSessionV1,
    overlay: GuestImageOverlayV1,
    scheduler_permit: SessionPermit,
    profile_id: Option<String>,
    initial_owner: crate::computer_use::mcp::ComputerUseControlOwnerV1,
    private_input: bool,
    cancelled: &AtomicBool,
    journal_guard: &mut StartupJournalGuard<'_>,
    startup_view: &mut StartupViewGuard<'_>,
) -> Result<(), &'static str> {
    if super::guards::record_session(runtime, session, caller, JournalEvent::OverlayPrepared)
        .is_err()
    {
        overlay.release_after_vm_stopped().ok();
        return Err("session_journal_unavailable");
    }
    if cancelled.load(Ordering::Acquire) {
        overlay.release_after_vm_stopped().ok();
        return Err("cancelled");
    }
    if super::guards::record_session(
        runtime,
        session,
        caller,
        JournalEvent::BackendPrepareStarted,
    )
    .is_err()
    {
        overlay.release_after_vm_stopped().ok();
        return Err("session_journal_unavailable");
    }
    let (handle, _) = runtime
        .backend
        .prepare(session, overlay, unix_time_ms())
        .map_err(|_| "backend_prepare_failed")?;
    if super::guards::record_session(runtime, session, caller, JournalEvent::BackendPrepared)
        .is_err()
    {
        super::super::cleanup(runtime, session, &handle);
        return Err("session_journal_unavailable");
    }
    if cancelled.load(Ordering::Acquire) {
        super::super::cleanup(runtime, session, &handle);
        return Err("cancelled");
    }
    if super::guards::record_session(runtime, session, caller, JournalEvent::BootStarted).is_err() {
        super::super::cleanup(runtime, session, &handle);
        return Err("session_journal_unavailable");
    }
    if runtime
        .backend
        .boot(session, &handle, unix_time_ms())
        .is_err()
    {
        super::super::cleanup(runtime, session, &handle);
        return Err("backend_boot_failed");
    }
    if cancelled.load(Ordering::Acquire) {
        super::super::cleanup(runtime, session, &handle);
        return Err("cancelled");
    }
    #[cfg(target_os = "linux")]
    let channel = match runtime
        .backend
        .authenticate_guest(session, &handle, unix_time_ms())
    {
        Ok(channel) => channel,
        Err(_) => {
            super::super::cleanup(runtime, session, &handle);
            return Err("guest_authentication_failed");
        }
    };
    #[cfg(not(target_os = "linux"))]
    {
        super::super::cleanup(runtime, session, &handle);
        return Err("unsupported_host");
    }
    if cancelled.load(Ordering::Acquire) {
        super::super::cleanup(runtime, session, &handle);
        return Err("cancelled");
    }
    let readiness = runtime.backend.wait_ready(
        session,
        &handle,
        std::time::Duration::from_secs(120),
        unix_time_ms(),
    );
    if let Err(error) = super::guards::record_guest_ready(runtime, session, caller, readiness) {
        super::super::cleanup(runtime, session, &handle);
        return Err(error);
    }
    if cancelled.load(Ordering::Acquire) {
        super::super::cleanup(runtime, session, &handle);
        return Err("cancelled");
    }
    let journal = match runtime
        .backend
        .open_guest_action_journal(session, &handle, unix_time_ms())
    {
        Ok(journal) => journal,
        Err(_) => {
            super::super::cleanup(runtime, session, &handle);
            return Err("journal_unavailable");
        }
    };
    let mut sessions = match controller.sessions.lock() {
        Ok(sessions) => sessions,
        Err(_) => {
            super::super::cleanup(runtime, session, &handle);
            return Err("session_state_unavailable");
        }
    };
    if sessions.contains_key(&caller.identity.session_id) {
        super::super::cleanup(runtime, session, &handle);
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
        super::super::cleanup(runtime, session, &handle);
        return Err("authority_registration_failed");
    }
    if cancelled.load(Ordering::Acquire) {
        controller.registry.revoke(&caller.identity.session_id).ok();
        super::super::cleanup(runtime, session, &handle);
        return Err("cancelled");
    }
    sessions.insert(
        caller.identity.session_id.clone(),
        ActiveSession {
            session: session.clone(),
            handle,
            journal,
            channel: Some(channel),
            started: true,
            paused: false,
            uncertain: false,
            uncertain_action_sequence: None,
            inspection_result: None,
            next_action_sequence: 1,
            role_id: caller.role_id.clone(),
            control_owner: initial_owner,
            control_generation: 1,
            require_fresh_agent_observation: false,
            private_input,
            active_app: None,
            personal_profile_id: profile_id,
            action_log: std::collections::VecDeque::new(),
            pending_action_intent: None,
            display_io: std::sync::Arc::new(std::sync::Mutex::new(())),
            _scheduler_permit: scheduler_permit,
        },
    );
    if let Some(active) = sessions.get(&caller.identity.session_id) {
        controller.cache_active_view(active);
    }
    journal_guard.mark_completed();
    startup_view.keep();
    Ok(())
}
