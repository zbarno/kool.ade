use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::computer_use::{
    ComputerActionV1, ComputerOperationV1,
    guest_protocol::{AuthorizedComputerUseSessionV1, GuestProtocolErrorV1},
};

use super::super::super::{ComputerUseCallerV1, ComputerUseMcpResponseV1};
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
}

pub(super) fn action(
    active: &mut ActiveSession,
    context: ActionContext<'_>,
    payload: crate::computer_use::ComputerActionPayloadV1,
    timeout_ms: u32,
) -> ComputerUseMcpResponseV1 {
    let ActionContext {
        controller,
        authority,
        caller,
        cancelled,
        session_id,
    } = context;
    if active.paused || !active.started {
        return controller.blocked("session_not_ready");
    }
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
    let action = ComputerActionV1 {
        schema_version: 1,
        session_id: session_id.to_owned(),
        action_sequence: sequence,
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        actor: caller.actor,
        lease_generation: active.session.lease.generation,
        approval_generation: active.session.approval_generation,
        cancellation_generation: active.session.cancellation_generation,
        operation,
        payload,
    };
    let session = active.session.clone();
    let handle = active.handle.clone();
    let result = run_cancellable(controller, &session, &handle, cancelled, || {
        authority.execute(
            channel,
            &active.journal,
            &action,
            deadline.min(active.session.lease.expires_at_unix_ms),
            now,
        )
    });
    if cancelled.load(Ordering::Acquire) {
        active.channel.take();
        active.uncertain = true;
        controller.cache_active_view(active);
        return denied("cancelled");
    }
    let response = match result {
        Ok(outcome) => {
            active.next_action_sequence = active.next_action_sequence.saturating_add(1);
            if let crate::computer_use::ComputerActionPayloadV1::LaunchApp { app_id } =
                &summary_payload
            {
                active.active_app = Some(app_id.clone());
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
    use super::should_record_action_summary;
    use crate::computer_use::{ComputerActionActorV1, ComputerOperationV1 as Operation};

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
