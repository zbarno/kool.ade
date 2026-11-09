use std::sync::Arc;

use super::super::types::ComputerUseMcpControllerV1;
use crate::computer_use::mcp::ComputerUseControlCommandV1 as ControlCommand;

impl ComputerUseMcpControllerV1 {
    pub fn queue_control_change(
        self: &Arc<Self>,
        task_id: &str,
        user_id: &str,
        session_id: &str,
        command: ControlCommand,
    ) -> Result<(), &'static str> {
        let view = self
            .view_cache
            .lock()
            .map_err(|_| "session_state_unavailable")?
            .get(session_id)
            .cloned()
            .ok_or("session_not_active")?;
        if view.task_id != task_id || view.user_id != user_id {
            return Err("session_binding_mismatch");
        }
        let is_active = self
            .sessions
            .lock()
            .map_err(|_| "session_state_unavailable")?
            .contains_key(session_id);
        let recovered = !is_active;
        if view.status == crate::computer_use::mcp::ComputerUseTaskSessionStatusV1::Uncertain
            && command == ControlCommand::InspectUncertainOutcome
            && recovered
        {
            let journal = &self
                .runtime
                .as_ref()
                .ok_or("trusted_runtime_unavailable")?
                .session_journal;
            let event = journal
                .latest_events()?
                .into_iter()
                .find(|event| event.identity.session_id == session_id)
                .ok_or("session_journal_record_not_found")?;
            let notice = match event.reconciled_from {
                Some(
                    super::super::session_journal::SessionEventKindV1::ActionIntent
                    | super::super::session_journal::SessionEventKindV1::OutcomeUnknown,
                ) => format!(
                    "Action {} was in flight when the app restarted. Its result is unknown; do not retry it.",
                    event.action_sequence.unwrap_or_default()
                ),
                Some(super::super::session_journal::SessionEventKindV1::ActionApplied) => format!(
                    "Action {} completed before the app restarted. The VM was stopped during recovery.",
                    event.action_sequence.unwrap_or_default()
                ),
                _ => "No uncertain action was recorded. The VM was stopped during recovery.".into(),
            };
            self.recovery_notices
                .lock()
                .map_err(|_| "session_state_unavailable")?
                .insert(session_id.to_owned(), notice);
            return Ok(());
        }
        if view.status == crate::computer_use::mcp::ComputerUseTaskSessionStatusV1::Interrupted
            && command == ControlCommand::StopComputer
            && recovered
        {
            self.runtime
                .as_ref()
                .ok_or("trusted_runtime_unavailable")?
                .session_journal
                .dismiss(session_id, super::super::unix_time_ms())?;
            self.recovery_notices
                .lock()
                .map_err(|_| "session_state_unavailable")?
                .remove(session_id);
            self.remove_cached_view(session_id);
            return Ok(());
        }
        if view.status == crate::computer_use::mcp::ComputerUseTaskSessionStatusV1::Queued
            && command == ControlCommand::StopComputer
        {
            let scheduler = self
                .runtime
                .as_ref()
                .ok_or("trusted_runtime_unavailable")?
                .scheduler
                .clone();
            if !scheduler.cancel_waiting(session_id) {
                return Err("queued_session_changed_state");
            }
            self.remove_cached_view(session_id);
            return Ok(());
        }
        if view.status != crate::computer_use::mcp::ComputerUseTaskSessionStatusV1::Active
            && view.status != crate::computer_use::mcp::ComputerUseTaskSessionStatusV1::Uncertain
            && view.status != crate::computer_use::mcp::ComputerUseTaskSessionStatusV1::Interrupted
        {
            return Err("session_starting");
        }
        {
            let mut pending = self
                .pending_controls
                .lock()
                .map_err(|_| "session_state_unavailable")?;
            if pending.get(session_id).is_some_and(Option::is_none) {
                return Err("control_change_already_pending");
            }
            pending.insert(session_id.to_owned(), None);
        }
        let weak = Arc::downgrade(self);
        let task = task_id.to_owned();
        let user = user_id.to_owned();
        let session = session_id.to_owned();
        std::thread::spawn(move || {
            let Some(controller) = weak.upgrade() else {
                return;
            };
            let result = match command {
                ControlCommand::PauseAgent => controller.pause_agent(&task, &user, &session),
                ControlCommand::ResumeAgent => controller.resume_agent(&task, &user, &session),
                ControlCommand::TakeControl => {
                    controller.take_human_control(&task, &user, &session)
                }
                ControlCommand::ReturnControl => {
                    controller.return_agent_control(&task, &user, &session)
                }
                ControlCommand::StopComputer => {
                    controller.stop_from_task_view(&task, &user, &session)
                }
                ControlCommand::InspectUncertainOutcome => {
                    controller.inspect_uncertain_outcome(&task, &user, &session)
                }
                ControlCommand::LaunchApprovedApp(app_id) => {
                    controller.launch_approved_app(&task, &user, &session, &app_id)
                }
            };
            let error = result.err().map(str::to_owned);
            if let Ok(mut pending) = controller.pending_controls.lock() {
                if let Some(error) = &error {
                    pending.insert(session.clone(), Some(error.clone()));
                } else {
                    pending.remove(&session);
                }
            }
            if let Some(error) = error
                && let Ok(mut views) = controller.view_cache.lock()
                && let Some(view) = views.get_mut(&session)
            {
                view.control_error = Some(error);
            }
        });
        Ok(())
    }
}
