use sha2::Digest;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::computer_use::{
    ComputerActionHelperPhaseV1, ComputerActionIntentStateV1 as IntentState, ComputerActionV1,
    ComputerOperationV1,
    guest_protocol::{AuthorizedComputerUseSessionV1, GuestProtocolErrorV1},
};

use super::super::super::{ComputerUseCallerV1, ComputerUseMcpResponseV1};
use super::super::session_journal::SessionEventKindV1 as JournalEvent;
use super::super::{
    cancellation::run_cancellable,
    session::{completed, denied, unix_time_ms},
    types::{ActiveSession, ComputerUseMcpControllerV1},
};

pub(super) struct ActionContext<'a> {
    pub(super) controller: &'a ComputerUseMcpControllerV1,
    pub(super) authority: Arc<AuthorizedComputerUseSessionV1>,
    pub(super) caller: &'a ComputerUseCallerV1,
    pub(super) cancelled: &'a AtomicBool,
    pub(super) session_id: &'a str,
    pub(super) staged_task_revision: Option<&'a str>,
}

pub(super) fn action(
    active: &mut ActiveSession,
    context: ActionContext<'_>,
    payload: crate::computer_use::ComputerActionPayloadV1,
    timeout_ms: u32,
    idempotency_key: Option<String>,
) -> ComputerUseMcpResponseV1 {
    let ActionContext {
        controller,
        authority,
        caller,
        cancelled,
        session_id,
        staged_task_revision,
    } = context;
    if let Some(reason) = action_blocker(active.uncertain, active.paused, active.started) {
        return controller.blocked(reason);
    }
    let sensitive_intent_id = if let Some(pending) = active.pending_action_intent.as_mut() {
        if caller.actor != crate::computer_use::ComputerActionActorV1::Agent
            || pending.state != IntentState::ApprovedOnce
            || pending.final_action.sha256().ok() != payload.sha256().ok()
        {
            return controller.blocked("sensitive_action_requires_exact_one_time_approval");
        }
        let now = unix_time_ms();
        if pending.intent.validate(now).is_err()
            || pending
                .approval
                .as_ref()
                .is_none_or(|approval| approval.validate_for(&pending.intent, now).is_err())
        {
            let digest = pending.intent.digest().unwrap_or_default();
            if controller.runtime.as_ref().is_none_or(|runtime| {
                runtime
                    .session_journal
                    .append_action_intent_state(
                        &active.session,
                        &active.role_id,
                        &pending.intent.intent_id,
                        &digest,
                        IntentState::Expired,
                        now,
                    )
                    .is_err()
            }) {
                return controller.blocked("session_journal_unavailable");
            }
            pending.state = IntentState::Expired;
            pending.approval = None;
            active.pending_action_intent = None;
            controller.cache_active_view(active);
            return controller.blocked("sensitive_action_approval_expired");
        }
        let frame_matches = controller
            .display_streams
            .lock()
            .ok()
            .and_then(|streams| streams.get(session_id).cloned())
            .and_then(|snapshot| {
                snapshot
                    .lock()
                    .ok()
                    .and_then(|snapshot| snapshot.frame_png.clone())
            })
            .is_some_and(|png| {
                format!("sha256:{:x}", sha2::Sha256::digest(&png)) == pending.intent.frame_sha256
            });
        if !frame_matches {
            let digest = pending.intent.digest().unwrap_or_default();
            if controller.runtime.as_ref().is_none_or(|runtime| {
                runtime
                    .session_journal
                    .append_action_intent_state(
                        &active.session,
                        &active.role_id,
                        &pending.intent.intent_id,
                        &digest,
                        IntentState::Cancelled,
                        now,
                    )
                    .is_err()
            }) {
                return controller.blocked("session_journal_unavailable");
            }
            pending.state = IntentState::Cancelled;
            pending.approval = None;
            active.pending_action_intent = None;
            controller.cache_active_view(active);
            return controller.blocked("action_intent_frame_stale");
        }
        pending.state = IntentState::Executing;
        pending.approval = None;
        let Some(runtime) = controller.runtime.as_ref() else {
            return controller.blocked("trusted_runtime_unavailable");
        };
        let digest = pending.intent.digest().unwrap_or_default();
        if runtime
            .session_journal
            .append_action_intent_state(
                &active.session,
                &active.role_id,
                &pending.intent.intent_id,
                &digest,
                IntentState::Executing,
                now,
            )
            .is_err()
        {
            pending.state = IntentState::OutcomeUnknown;
            controller.cache_active_view(active);
            return controller.blocked("session_journal_unavailable");
        }
        Some(pending.intent.intent_id.clone())
    } else {
        None
    };
    let Some(channel) = active.channel.as_mut() else {
        active.uncertain = true;
        controller.cache_active_view(active);
        return controller.blocked("guest_channel_unavailable");
    };
    let operation = operation(&payload);
    let summary_payload = payload.clone();
    let now = unix_time_ms();
    let Some(deadline) = now.checked_add(timeout_ms as u64) else {
        return denied("invalid_request");
    };
    let sequence = active.next_action_sequence;
    if sequence > active.session.policy.resource_budget.action_limit as u64 {
        return controller.blocked("action_budget_exhausted");
    }
    let mut action = ComputerActionV1 {
        schema_version: 1,
        session_id: session_id.to_owned(),
        action_sequence: sequence,
        idempotency_key: idempotency_key.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        actor: caller.actor,
        lease_generation: active.session.lease.generation,
        approval_generation: active.session.approval_generation,
        cancellation_generation: active.session.cancellation_generation,
        operation,
        payload,
        helper_authorization: None,
    };
    let mut staged_transfer_action = None;
    if staged_task_revision.is_some()
        || operation == crate::computer_use::ComputerOperationV1::ManageApp
    {
        let Some(runtime) = controller.runtime.as_ref() else {
            return controller.blocked("trusted_runtime_unavailable");
        };
        if staged_task_revision.is_some() {
            let mut transfer_action = action.clone();
            if transfer_action
                .sign_for_guest_helper(
                    &runtime.controller_key_id,
                    ComputerActionHelperPhaseV1::Stage,
                    &runtime.controller_key,
                )
                .is_err()
                || action
                    .sign_for_guest_helper(
                        &runtime.controller_key_id,
                        ComputerActionHelperPhaseV1::Launch,
                        &runtime.controller_key,
                    )
                    .is_err()
            {
                return controller.blocked("guest_helper_authorization_failed");
            }
            staged_transfer_action = Some(transfer_action);
        } else if action
            .sign_for_guest_helper(
                &runtime.controller_key_id,
                ComputerActionHelperPhaseV1::Manage,
                &runtime.controller_key,
            )
            .is_err()
        {
            return controller.blocked("guest_helper_authorization_failed");
        }
    }
    active.inspection_result = None;
    let session = active.session.clone();
    let handle = active.handle.clone();
    let Some(runtime) = controller.runtime.as_ref() else {
        return controller.blocked("trusted_runtime_unavailable");
    };
    if runtime
        .session_journal
        .append_session(
            &session,
            &active.role_id,
            JournalEvent::ActionIntent,
            Some(&action),
            now,
        )
        .is_err()
    {
        return controller.blocked("session_journal_unavailable");
    }
    let result = run_cancellable(controller, &session, &handle, cancelled, || {
        if let Some(current_task_revision) = staged_task_revision {
            authority.execute_staged_task_application(
                channel,
                &active.journal,
                &runtime.artifact_bridge,
                &action,
                staged_transfer_action
                    .as_ref()
                    .ok_or(GuestProtocolErrorV1::Malformed)?,
                current_task_revision,
                deadline.min(active.session.lease.expires_at_unix_ms),
                now,
            )
        } else {
            authority.execute(
                channel,
                &active.journal,
                &action,
                deadline.min(active.session.lease.expires_at_unix_ms),
                now,
            )
        }
    });
    let result_kind = match &result {
        Ok(_) => JournalEvent::ActionApplied,
        Err(
            GuestProtocolErrorV1::OutcomeUnknown
            | GuestProtocolErrorV1::Io
            | GuestProtocolErrorV1::GuestExited(_),
        ) => JournalEvent::OutcomeUnknown,
        Err(_) => JournalEvent::ActionFailed,
    };
    if let Some(intent_id) = &sensitive_intent_id
        && let Some(pending) = active.pending_action_intent.as_mut()
        && pending.intent.intent_id == *intent_id
        && result.is_err()
    {
        pending.state = IntentState::OutcomeUnknown;
        let digest = pending.intent.digest().unwrap_or_default();
        let _ = runtime.session_journal.append_action_intent_state(
            &session,
            &active.role_id,
            intent_id,
            &digest,
            IntentState::OutcomeUnknown,
            unix_time_ms(),
        );
    }
    if runtime
        .session_journal
        .append_session(
            &session,
            &active.role_id,
            result_kind,
            Some(&action),
            unix_time_ms(),
        )
        .is_err()
    {
        active.channel.take();
        active.uncertain = true;
        active.uncertain_action_sequence = Some(sequence);
        controller.cache_active_view(active);
        return controller.blocked("session_journal_unavailable");
    }
    if cancelled.load(Ordering::Acquire) {
        active.channel.take();
        active.uncertain = true;
        active.uncertain_action_sequence = Some(sequence);
        controller.cache_active_view(active);
        return denied("cancelled");
    }
    let response = match result {
        Ok(outcome) => {
            active.next_action_sequence = active.next_action_sequence.saturating_add(1);
            active.uncertain_action_sequence = None;
            match &summary_payload {
                crate::computer_use::ComputerActionPayloadV1::LaunchApp { app_id } => {
                    active.active_app = Some(app_id.clone());
                }
                crate::computer_use::ComputerActionPayloadV1::LaunchApprovedStagedTaskApp {
                    application,
                } => active.active_app = Some(application.app_id.clone()),
                _ => {}
            }
            let label = action_label(&summary_payload);
            if should_record_action_summary(active.private_input, caller.actor, operation) {
                active
                    .action_log
                    .push_back(crate::computer_use::mcp::ComputerUseActionSummaryV1 {
                        actor: if caller.actor == crate::computer_use::ComputerActionActorV1::Human
                        {
                            crate::computer_use::mcp::ComputerUseControlOwnerV1::Human
                        } else {
                            crate::computer_use::mcp::ComputerUseControlOwnerV1::Agent
                        },
                        label,
                        occurred_at_unix_ms: now,
                    });
                while active.action_log.len() > 50 {
                    active.action_log.pop_front();
                }
            }
            let mut result = completed(session_id);
            result.observation = outcome.observation.clone();
            result.action_outcome = Some(outcome);
            result
        }
        Err(
            GuestProtocolErrorV1::OutcomeUnknown
            | GuestProtocolErrorV1::Io
            | GuestProtocolErrorV1::GuestExited(_),
        ) => {
            active.next_action_sequence = active.next_action_sequence.saturating_add(1);
            active.uncertain = true;
            active.uncertain_action_sequence = Some(sequence);
            controller.blocked("action_outcome_unknown")
        }
        Err(GuestProtocolErrorV1::UnsupportedCapability) => {
            controller.blocked("guest_capability_unavailable")
        }
        Err(GuestProtocolErrorV1::StaleObservation) => denied("stale_observation"),
        Err(GuestProtocolErrorV1::DeadlineExpired) => denied("action_deadline_expired"),
        Err(_) => denied("action_authorization_failed"),
    };
    controller.cache_active_view(active);
    response
}

fn action_blocker(uncertain: bool, paused: bool, started: bool) -> Option<&'static str> {
    if uncertain {
        Some("session_state_uncertain")
    } else if paused || !started {
        Some("session_not_ready")
    } else {
        None
    }
}

fn action_label(payload: &crate::computer_use::ComputerActionPayloadV1) -> String {
    use crate::computer_use::ComputerActionPayloadV1 as P;
    match payload {
        P::ObserveScreen => "Observed desktop".into(),
        P::ObserveWindows => "Observed open windows".into(),
        P::ReadAccessibility => "Read accessibility information".into(),
        P::MouseClick { .. } | P::MouseDoubleClick { .. } => "Used pointer".into(),
        P::MouseMove { .. } => "Moved pointer".into(),
        P::MouseDrag { .. } => "Dragged pointer".into(),
        P::MouseScroll { .. } => "Scrolled desktop".into(),
        P::FocusWindow { .. } => "Focused a window".into(),
        P::KeyPress { .. } | P::KeyChord { .. } => "Used keyboard".into(),
        P::TypeText { .. } => "Entered text".into(),
        P::LaunchApp { .. } => "Launched an approved app".into(),
        P::ManageApp { .. } => "Managed an approved guest app".into(),
        P::LaunchApprovedStagedTaskApp { .. } => "Launched an approved staged task app".into(),
        P::WaitFor { .. } => "Waited for the desktop".into(),
        P::QueryState => "Checked desktop status".into(),
    }
}

pub(crate) fn operation(
    payload: &crate::computer_use::ComputerActionPayloadV1,
) -> ComputerOperationV1 {
    use crate::computer_use::ComputerActionPayloadV1 as P;
    match payload {
        P::ObserveScreen => ComputerOperationV1::ObserveScreen,
        P::ObserveWindows => ComputerOperationV1::ObserveWindows,
        P::ReadAccessibility => ComputerOperationV1::ReadAccessibility,
        P::MouseClick { .. }
        | P::MouseDoubleClick { .. }
        | P::MouseMove { .. }
        | P::MouseDrag { .. }
        | P::MouseScroll { .. }
        | P::FocusWindow { .. } => ComputerOperationV1::MouseInput,
        P::KeyPress { .. } | P::KeyChord { .. } | P::TypeText { .. } => {
            ComputerOperationV1::KeyboardInput
        }
        P::LaunchApp { .. } => ComputerOperationV1::LaunchAllowedApp,
        P::ManageApp { .. } => ComputerOperationV1::ManageApp,
        P::LaunchApprovedStagedTaskApp { .. } => ComputerOperationV1::LaunchApprovedStagedTaskApp,
        P::WaitFor { .. } => ComputerOperationV1::WaitFor,
        P::QueryState => ComputerOperationV1::QueryState,
    }
}

fn should_record_action_summary(
    private_input: bool,
    actor: crate::computer_use::ComputerActionActorV1,
    operation: ComputerOperationV1,
) -> bool {
    !(private_input
        && actor == crate::computer_use::ComputerActionActorV1::Human
        && operation == ComputerOperationV1::KeyboardInput)
}

#[cfg(test)]
mod tests {
    use super::{action_blocker, should_record_action_summary};
    use crate::computer_use::{ComputerActionActorV1, ComputerOperationV1 as Operation};

    #[test]
    fn uncertain_action_outcome_fences_every_following_action() {
        assert_eq!(
            action_blocker(true, false, true),
            Some("session_state_uncertain")
        );
        assert_eq!(action_blocker(false, false, true), None);
        assert_eq!(action_blocker(false, true, true), Some("session_not_ready"));
        assert_eq!(
            action_blocker(false, false, false),
            Some("session_not_ready")
        );
    }

    #[test]
    fn private_human_keyboard_actions_are_omitted_from_the_local_journal() {
        assert!(!should_record_action_summary(
            true,
            ComputerActionActorV1::Human,
            Operation::KeyboardInput,
        ));
        assert!(should_record_action_summary(
            true,
            ComputerActionActorV1::Human,
            Operation::MouseInput,
        ));
        assert!(should_record_action_summary(
            true,
            ComputerActionActorV1::Agent,
            Operation::KeyboardInput,
        ));
        assert!(should_record_action_summary(
            false,
            ComputerActionActorV1::Human,
            Operation::KeyboardInput,
        ));
    }
}
