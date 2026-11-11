use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::computer_use::guest_protocol::AuthorizedComputerUseSessionV1;

use super::super::{
    ComputerUseCallerV1, ComputerUseMcpRequestV1, ComputerUseMcpResponseV1,
    ComputerUseObservationKindV1,
};
use super::{
    cancellation::run_cancellable,
    session::{completed, denied, failed, unix_time_ms},
    types::ComputerUseMcpControllerV1,
};

mod action;
mod action_intent;
#[cfg(test)]
mod personal_handoff_tests;
#[cfg(test)]
pub(super) use action::operation;
use action::{ActionContext, action};
pub(super) use action_intent::propose_sensitive_action;

pub(super) fn staged_task_action(
    controller: &ComputerUseMcpControllerV1,
    active: &mut super::types::ActiveSession,
    authority: Arc<AuthorizedComputerUseSessionV1>,
    caller: &ComputerUseCallerV1,
    session_id: &str,
    application: crate::computer_use::apps::StagedTaskApplicationV1,
    cancelled: &AtomicBool,
) -> ComputerUseMcpResponseV1 {
    let operation = crate::computer_use::ComputerOperationV1::LaunchApprovedStagedTaskApp;
    if !active
        .session
        .policy
        .authorized_operations
        .contains(&operation)
    {
        return denied("operation_not_authorized");
    }
    let idempotency_key = uuid::Uuid::new_v5(
        &uuid::Uuid::NAMESPACE_URL,
        format!(
            "computer-use-qa-app:{}:{}:{}",
            session_id, application.build_approval_sha256, application.build_receipt_sha256
        )
        .as_bytes(),
    )
    .to_string();
    let staged_task_revision = application.task_revision.clone();
    action(
        active,
        ActionContext {
            controller,
            authority,
            caller,
            cancelled,
            session_id,
            staged_task_revision: Some(&staged_task_revision),
        },
        crate::computer_use::ComputerActionPayloadV1::LaunchApprovedStagedTaskApp {
            application: Box::new(application),
        },
        60_000,
        Some(idempotency_key),
    )
}

pub(super) fn dispatch(
    controller: &ComputerUseMcpControllerV1,
    caller: &ComputerUseCallerV1,
    authority: Arc<AuthorizedComputerUseSessionV1>,
    request: ComputerUseMcpRequestV1,
    cancelled: &AtomicBool,
) -> ComputerUseMcpResponseV1 {
    let Some(runtime) = controller.runtime.as_ref() else {
        return controller.blocked(
            controller
                .unavailable_reason
                .as_deref()
                .unwrap_or("trusted_runtime_unavailable"),
        );
    };
    if cancelled.load(Ordering::Acquire) {
        return denied("cancelled");
    }
    let session_id = match &request {
        ComputerUseMcpRequestV1::Capabilities | ComputerUseMcpRequestV1::RequestSession { .. } => {
            return denied("invalid_request");
        }
        ComputerUseMcpRequestV1::StartSession { session_id }
        | ComputerUseMcpRequestV1::Observe { session_id, .. }
        | ComputerUseMcpRequestV1::Action { session_id, .. }
        | ComputerUseMcpRequestV1::ProposeSensitiveAction { session_id, .. }
        | ComputerUseMcpRequestV1::Pause { session_id }
        | ComputerUseMcpRequestV1::Resume { session_id }
        | ComputerUseMcpRequestV1::Release { session_id } => session_id.clone(),
    };
    if caller.actor == crate::computer_use::ComputerActionActorV1::Agent
        && controller.control_change_pending(&session_id)
    {
        return controller.blocked("control_transition_in_progress");
    }
    if session_id != caller.identity.session_id || authority.session().identity != caller.identity {
        return denied("authorization_failed");
    }
    let Ok(mut sessions) = controller.sessions.lock() else {
        return failed("session_state_unavailable");
    };
    let Some(active) = sessions.get_mut(&session_id) else {
        return denied("session_not_active");
    };
    if caller.actor == crate::computer_use::ComputerActionActorV1::Agent
        && controller.control_change_pending(&session_id)
    {
        return controller.blocked("control_transition_in_progress");
    }
    if active.session.identity != caller.identity || active.uncertain {
        return controller.blocked("session_state_uncertain");
    }
    match (caller.actor, active.control_owner) {
        (
            crate::computer_use::ComputerActionActorV1::Agent,
            crate::computer_use::mcp::ComputerUseControlOwnerV1::Agent,
        )
        | (
            crate::computer_use::ComputerActionActorV1::Human,
            crate::computer_use::mcp::ComputerUseControlOwnerV1::Human,
        ) => {}
        (crate::computer_use::ComputerActionActorV1::Agent, _) => {
            return controller.blocked("agent_control_not_owned");
        }
        (crate::computer_use::ComputerActionActorV1::Human, _) => {
            return denied("human_control_not_owned");
        }
    }
    if handback_requires_fresh_screenshot(
        caller.actor,
        active.require_fresh_agent_observation,
        &request,
    ) {
        return controller.blocked("fresh_observation_required_after_handback");
    }
    match request {
        ComputerUseMcpRequestV1::StartSession { .. } => {
            if active.paused {
                return controller.blocked("session_paused");
            }
            completed(&session_id)
        }
        ComputerUseMcpRequestV1::Observe { observation, .. } => {
            let payload = match observation {
                ComputerUseObservationKindV1::Screenshot => {
                    crate::computer_use::ComputerActionPayloadV1::ObserveScreen
                }
                ComputerUseObservationKindV1::Windows => {
                    crate::computer_use::ComputerActionPayloadV1::ObserveWindows
                }
                ComputerUseObservationKindV1::State => {
                    crate::computer_use::ComputerActionPayloadV1::QueryState
                }
                ComputerUseObservationKindV1::Accessibility => {
                    return controller.blocked("guest_accessibility_unavailable");
                }
            };
            let result = action(
                active,
                ActionContext {
                    controller,
                    authority,
                    caller,
                    cancelled,
                    session_id: &session_id,
                    staged_task_revision: None,
                },
                payload,
                60_000,
                None,
            );
            if caller.actor == crate::computer_use::ComputerActionActorV1::Agent
                && observation == ComputerUseObservationKindV1::Screenshot
                && result.status == super::super::ComputerUseMcpStatusV1::Completed
                && result.observation.is_some()
            {
                active.require_fresh_agent_observation = false;
            }
            result
        }
        ComputerUseMcpRequestV1::Action {
            payload,
            timeout_ms,
            idempotency_key,
            ..
        } => action(
            active,
            ActionContext {
                controller,
                authority,
                caller,
                cancelled,
                session_id: &session_id,
                staged_task_revision: None,
            },
            payload,
            timeout_ms,
            idempotency_key,
        ),
        ComputerUseMcpRequestV1::ProposeSensitiveAction {
            intent,
            final_action,
            ..
        } => propose_sensitive_action(
            controller,
            active,
            caller,
            &session_id,
            &intent,
            &final_action,
        ),
        ComputerUseMcpRequestV1::Pause { .. } => {
            if active.paused {
                return completed(&session_id);
            }
            if runtime
                .backend
                .suspend(&active.session, &active.handle, unix_time_ms())
                .is_err()
            {
                active.uncertain = true;
                return failed("backend_pause_failed");
            }
            active.channel.take();
            active.paused = true;
            controller.cache_active_view(active);
            completed(&session_id)
        }
        ComputerUseMcpRequestV1::Resume { .. } => {
            if !active.paused {
                return completed(&session_id);
            }
            let session = active.session.clone();
            let handle = active.handle.clone();
            let resumed = run_cancellable(
                controller,
                &session,
                &handle,
                cancelled,
                || -> Result<_, &'static str> {
                    runtime
                        .backend
                        .resume(&session, &handle, unix_time_ms())
                        .map_err(|_| "backend_resume_failed")?;
                    #[cfg(target_os = "linux")]
                    let channel = runtime
                        .backend
                        .authenticate_guest(&session, &handle, unix_time_ms())
                        .map_err(|_| "guest_reauthentication_failed")?;
                    #[cfg(not(target_os = "linux"))]
                    return Err("unsupported_host");
                    runtime
                        .backend
                        .wait_ready(
                            &session,
                            &handle,
                            std::time::Duration::from_secs(120),
                            unix_time_ms(),
                        )
                        .map_err(|_| "guest_not_ready")?;
                    #[cfg(target_os = "linux")]
                    Ok(channel)
                },
            );
            if cancelled.load(Ordering::Acquire) {
                active.channel.take();
                active.uncertain = true;
                return denied("cancelled");
            }
            match resumed {
                Ok(channel) => {
                    active.channel = Some(channel);
                    active.paused = false;
                    controller.cache_active_view(active);
                    completed(&session_id)
                }
                Err("backend_resume_failed") => {
                    active.uncertain = true;
                    failed("backend_resume_failed")
                }
                Err("guest_reauthentication_failed") => {
                    active.uncertain = true;
                    controller.blocked("guest_reauthentication_failed")
                }
                Err("guest_not_ready") => {
                    active.uncertain = true;
                    controller.blocked("guest_not_ready")
                }
                Err(_) => {
                    active.uncertain = true;
                    controller.blocked("unsupported_host")
                }
            }
        }
        ComputerUseMcpRequestV1::Release { .. } => {
            let active = sessions
                .remove(&session_id)
                .expect("active session was checked");
            controller.remove_cached_view(&session_id);
            let revoked = controller.registry.revoke(&session_id);
            let released = runtime
                .backend
                .release(&active.session, &active.handle, unix_time_ms())
                .is_err();
            if released {
                failed("backend_release_failed")
            } else if revoked.is_err() {
                failed("authority_revocation_failed")
            } else {
                completed(&session_id)
            }
        }
        ComputerUseMcpRequestV1::Capabilities | ComputerUseMcpRequestV1::RequestSession { .. } => {
            denied("invalid_request")
        }
    }
}

fn handback_requires_fresh_screenshot(
    actor: crate::computer_use::ComputerActionActorV1,
    required: bool,
    request: &ComputerUseMcpRequestV1,
) -> bool {
    actor == crate::computer_use::ComputerActionActorV1::Agent
        && required
        && !matches!(
            request,
            ComputerUseMcpRequestV1::Observe {
                observation: ComputerUseObservationKindV1::Screenshot,
                ..
            }
        )
}
