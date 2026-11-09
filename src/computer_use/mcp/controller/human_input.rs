use std::sync::atomic::AtomicBool;

use crate::computer_use::mcp::{
    ComputerUseCallerV1 as Caller, ComputerUseControlOwnerV1 as Owner,
    ComputerUseMcpRequestV1 as Request, ComputerUseMcpStatusV1 as Status,
};
use crate::computer_use::{
    ComputerActionActorV1 as Actor, ComputerActionPayloadV1 as Payload,
    ComputerDisplayInputV1 as DisplayInput, ComputerOperationV1 as Operation,
};

use super::{
    human_control::{bound_active, record},
    session::unix_time_ms,
    types::ComputerUseMcpControllerV1,
};

mod manage_app;
mod staged_task;

impl ComputerUseMcpControllerV1 {
    pub fn set_private_input(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        enabled: bool,
    ) -> Result<(), &'static str> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        let active = bound_active(&mut sessions, task_id, user_id, session_id)?;
        if active.control_owner != Owner::Human {
            return Err("human_control_is_not_active");
        }
        active.private_input = enabled;
        if enabled
            && let Ok(streams) = self.display_streams.lock()
            && let Some(snapshot) = streams.get(session_id)
            && let Ok(mut snapshot) = snapshot.lock()
        {
            snapshot.frame_png = None;
        }
        record(
            active,
            Owner::Human,
            if enabled {
                "Private input mode enabled"
            } else {
                "Private input mode disabled"
            },
            false,
        );
        self.cache_active_view(active);
        Ok(())
    }

    pub fn send_human_display_input(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        input: DisplayInput,
    ) -> Result<(), &'static str> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        let active = bound_active(&mut sessions, task_id, user_id, session_id)?;
        if active.control_owner != Owner::Human || active.uncertain || active.paused {
            return Err("human_control_is_not_ready");
        }
        let operation = match &input {
            DisplayInput::Pointer { .. } => Operation::MouseInput,
            DisplayInput::Key { .. } => Operation::KeyboardInput,
            DisplayInput::Text { .. } => Operation::KeyboardInput,
        };
        if !active
            .session
            .policy
            .authorized_operations
            .contains(&operation)
        {
            return Err("input_operation_not_authorized");
        }
        let runtime = self.runtime.as_ref().ok_or("trusted_runtime_unavailable")?;
        let _display_guard = active
            .display_io
            .lock()
            .map_err(|_| "display_state_unavailable")?;
        runtime
            .backend
            .send_display_input(&active.session, &active.handle, &input, unix_time_ms())
            .map_err(|_| "display_input_failed")?;
        drop(_display_guard);
        if !active.private_input {
            record(
                active,
                Owner::Human,
                match &input {
                    DisplayInput::Pointer { .. } => "Human used pointer",
                    DisplayInput::Key { .. } => "Human used keyboard",
                    DisplayInput::Text { .. } => "Human entered text",
                },
                false,
            );
        }
        self.cache_active_view(active);
        Ok(())
    }

    pub fn launch_approved_app(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        app_id: &str,
    ) -> Result<(), &'static str> {
        let caller = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|_| "session_state_unavailable")?;
            let active = sessions.get(session_id).ok_or("session_not_active")?;
            if active.session.identity.task_id != task_id
                || active.session.identity.user_id != user_id
                || active.control_owner != Owner::Human
                || !active.session.allowed_apps.contains(app_id)
                || !active
                    .session
                    .policy
                    .authorized_operations
                    .contains(&Operation::LaunchAllowedApp)
            {
                return Err("approved_app_not_available");
            }
            Caller {
                identity: active.session.identity.clone(),
                actor: Actor::Human,
                role_id: active.role_id.clone(),
            }
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
        let response = super::dispatch::dispatch(
            self,
            &caller,
            authority,
            Request::Action {
                session_id: session_id.to_owned(),
                payload: Payload::LaunchApp {
                    app_id: app_id.to_owned(),
                },
                timeout_ms: 30_000,
                idempotency_key: None,
            },
            &AtomicBool::new(false),
        );
        if response.status == Status::Completed {
            Ok(())
        } else {
            Err("approved_app_launch_failed")
        }
    }

    pub fn stop_from_task_view(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
    ) -> Result<(), &'static str> {
        let runtime = self.runtime.as_ref().ok_or("trusted_runtime_unavailable")?;
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        bound_active(&mut sessions, task_id, user_id, session_id)?;
        self.registry
            .revoke(session_id)
            .map_err(|_| "authority_revocation_failed")?;
        let active = sessions.remove(session_id).ok_or("session_not_active")?;
        self.remove_cached_view(session_id);
        let cancel = runtime
            .backend
            .cancel(&active.session, &active.handle, unix_time_ms());
        let release = runtime
            .backend
            .release(&active.session, &active.handle, unix_time_ms());
        if cancel.is_err() || release.is_err() {
            return Err("computer_cleanup_failed");
        }
        runtime
            .session_journal
            .append_session(
                &active.session,
                &active.role_id,
                super::session_journal::SessionEventKindV1::SessionStopped,
                None,
                unix_time_ms(),
            )
            .map_err(|_| "session_journal_unavailable")?;
        Ok(())
    }

    pub fn inspect_uncertain_outcome(
        &self,
        task_id: &str,
        user_id: &str,
        session_id: &str,
    ) -> Result<(), &'static str> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?;
        let active = bound_active(&mut sessions, task_id, user_id, session_id)?;
        if !active.uncertain {
            return Err("session_outcome_is_not_uncertain");
        }
        let detail = match active.uncertain_action_sequence {
            Some(sequence) => match active.journal.status_for_sequence(sequence) {
                Ok(crate::computer_use::guest_protocol::GuestActionJournalStatusV1::Pending) => {
                    format!("Action {sequence} remains possibly applied; do not retry it.")
                }
                Ok(crate::computer_use::guest_protocol::GuestActionJournalStatusV1::Completed(
                    result,
                )) => {
                    format!(
                        "Action {sequence} has a durable guest result ({result:?}); session remains fenced."
                    )
                }
                Ok(
                    crate::computer_use::guest_protocol::GuestActionJournalStatusV1::NotRecorded,
                ) => {
                    format!("Action {sequence} has no durable outcome; session remains fenced.")
                }
                Err(_) => "The local action journal is unavailable; session remains fenced.".into(),
            },
            None => {
                "No action sequence was recorded; session remains fenced until it is destroyed."
                    .into()
            }
        };
        active.inspection_result = Some(detail.clone());
        record(active, Owner::Paused, &detail, false);
        self.cache_active_view(active);
        Ok(())
    }
}
