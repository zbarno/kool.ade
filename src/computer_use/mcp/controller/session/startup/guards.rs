use crate::computer_use::ComputerSessionV1;

use super::super::super::session_journal::SessionEventKindV1 as Event;
use super::super::super::types::{ComputerUseMcpControllerV1, Runtime};
use super::super::super::unix_time_ms;

pub(super) struct StartupViewGuard<'a> {
    controller: &'a ComputerUseMcpControllerV1,
    session_id: String,
    keep: bool,
}

impl<'a> StartupViewGuard<'a> {
    pub fn new(controller: &'a ComputerUseMcpControllerV1, session_id: &str) -> Self {
        Self {
            controller,
            session_id: session_id.to_owned(),
            keep: false,
        }
    }

    pub fn keep(&mut self) {
        self.keep = true;
    }
}

impl Drop for StartupViewGuard<'_> {
    fn drop(&mut self) {
        if !self.keep {
            self.controller.remove_cached_view(&self.session_id);
        }
    }
}

pub(super) struct StartupJournalGuard<'a> {
    runtime: &'a Runtime,
    session: &'a ComputerSessionV1,
    role_id: &'a str,
    completed: bool,
}

impl<'a> StartupJournalGuard<'a> {
    pub fn new(runtime: &'a Runtime, session: &'a ComputerSessionV1, role_id: &'a str) -> Self {
        Self {
            runtime,
            session,
            role_id,
            completed: false,
        }
    }

    pub fn mark_completed(&mut self) {
        self.completed = true;
    }
}

impl Drop for StartupJournalGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.runtime
                .session_journal
                .append_session(
                    self.session,
                    self.role_id,
                    Event::StartupFailed,
                    None,
                    unix_time_ms(),
                )
                .ok();
        }
    }
}

pub(super) fn record_session(
    runtime: &super::super::super::types::Runtime,
    session: &crate::computer_use::ComputerSessionV1,
    caller: &crate::computer_use::mcp::ComputerUseCallerV1,
    kind: super::super::super::session_journal::SessionEventKindV1,
) -> Result<(), &'static str> {
    runtime
        .session_journal
        .append_session(
            session,
            &caller.role_id,
            kind,
            None,
            super::super::unix_time_ms(),
        )
        .map(|_| ())
}

pub(super) fn record_guest_ready(
    runtime: &super::super::super::types::Runtime,
    session: &crate::computer_use::ComputerSessionV1,
    caller: &crate::computer_use::mcp::ComputerUseCallerV1,
    readiness: Result<
        crate::computer_use::ComputerBackendEventV1,
        crate::computer_use::ComputerBackendErrorV1,
    >,
) -> Result<(), &'static str> {
    let event = readiness.map_err(|_| "guest_not_ready")?;
    if event.kind != crate::computer_use::ComputerBackendEventKindV1::GuestReady {
        return Err("guest_not_ready");
    }
    let boot_fingerprint = event
        .boot_fingerprint
        .ok_or("guest_boot_fingerprint_missing")?;
    runtime
        .session_journal
        .append_session_with_boot_fingerprint(
            session,
            &caller.role_id,
            super::super::super::session_journal::SessionEventKindV1::GuestReady,
            None,
            super::super::unix_time_ms(),
            Some(boot_fingerprint),
        )
        .map(|_| ())
        .map_err(|_| "session_journal_unavailable")
}
