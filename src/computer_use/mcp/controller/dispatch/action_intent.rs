use sha2::Digest;

use crate::computer_use::{
    ComputerActionActorV1, ComputerActionIntentStateV1, ComputerActionIntentV1,
    ComputerActionPayloadV1, ComputerActionRiskV1,
};

use crate::computer_use::mcp::{
    ComputerUseCallerV1, ComputerUseMcpResponseV1, ComputerUseMcpStatusV1,
};

use super::super::{
    session::unix_time_ms,
    types::{ActiveSession, ComputerUseMcpControllerV1},
};

pub(crate) fn propose_sensitive_action(
    controller: &ComputerUseMcpControllerV1,
    active: &mut ActiveSession,
    caller: &ComputerUseCallerV1,
    session_id: &str,
    intent: &ComputerActionIntentV1,
    final_action: &ComputerActionPayloadV1,
) -> ComputerUseMcpResponseV1 {
    let now = unix_time_ms();
    let session = &active.session;
    if caller.actor != ComputerActionActorV1::Agent
        || active.control_owner != crate::computer_use::mcp::ComputerUseControlOwnerV1::Agent
        || active.paused
        || active.pending_action_intent.is_some()
        || intent.operator_id != session.identity.user_id
        || intent.task_id != session.identity.task_id
        || intent.session_id != session_id
        || intent.lease_generation != session.lease.generation
        || intent.approval_generation != session.approval_generation
        || intent.cancellation_generation != session.cancellation_generation
        || intent.risk == ComputerActionRiskV1::Unclassified
        || is_non_committing_action(final_action)
        || final_action.sha256().ok().as_deref() != Some(intent.action_sha256.as_str())
        || intent.validate(now).is_err()
    {
        return denied("invalid_action_intent");
    }

    let current_frame_matches = controller
        .view_cache
        .lock()
        .ok()
        .and_then(|views| views.get(session_id).cloned())
        .and_then(|view| {
            let png = view.frame_png?;
            let digest = format!("sha256:{:x}", sha2::Sha256::digest(png));
            Some(view.frame_revision == intent.frame_revision && digest == intent.frame_sha256)
        })
        .unwrap_or(false);
    if !current_frame_matches {
        return denied("action_intent_frame_stale");
    }

    let digest = match intent.digest() {
        Ok(digest) => digest,
        Err(_) => return denied("invalid_action_intent"),
    };
    let Some(runtime) = controller.runtime.as_ref() else {
        return controller.blocked("trusted_runtime_unavailable");
    };
    for state in [
        ComputerActionIntentStateV1::Prepared,
        ComputerActionIntentStateV1::AwaitingApproval,
    ] {
        if runtime
            .session_journal
            .append_action_intent_state(
                session,
                &active.role_id,
                &intent.intent_id,
                &digest,
                state,
                now,
            )
            .is_err()
        {
            return controller.blocked("session_journal_unavailable");
        }
    }
    active.pending_action_intent = Some(super::super::types::PendingComputerActionIntentV1 {
        intent: intent.clone(),
        final_action: final_action.clone(),
        state: ComputerActionIntentStateV1::AwaitingApproval,
        approval: None,
    });
    controller.cache_active_view(active);
    ComputerUseMcpResponseV1 {
        schema_version: crate::computer_use::mcp::COMPUTER_USE_MCP_VERSION_V1,
        status: ComputerUseMcpStatusV1::Blocked,
        summary: "Sensitive action is waiting for exact human approval in Task Details.".into(),
        reason_code: Some("awaiting_human_action_approval".into()),
        session_id: Some(session_id.to_owned()),
        capabilities: Vec::new(),
        observation: None,
        action_outcome: None,
    }
}

fn is_non_committing_action(action: &ComputerActionPayloadV1) -> bool {
    matches!(
        action,
        ComputerActionPayloadV1::ObserveScreen
            | ComputerActionPayloadV1::ObserveWindows
            | ComputerActionPayloadV1::ReadAccessibility
            | ComputerActionPayloadV1::MouseMove { .. }
            | ComputerActionPayloadV1::WaitFor { .. }
            | ComputerActionPayloadV1::QueryState
    )
}

fn denied(reason: &str) -> ComputerUseMcpResponseV1 {
    super::super::session::denied(reason)
}
