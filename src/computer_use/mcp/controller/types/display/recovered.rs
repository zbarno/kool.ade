use super::super::ComputerUseMcpControllerV1;
use crate::computer_use::mcp::controller::session_journal::SessionEventKindV1;
use crate::computer_use::mcp::controller::unix_time_ms;
use crate::computer_use::mcp::{
    ComputerUseActionSummaryV1, ComputerUseControlOwnerV1,
    ComputerUseTaskSessionStatusV1 as Status, ComputerUseTaskSessionViewV1,
};
use crate::computer_use::{ComputerActionActorV1, ComputerLifecycleV1};

pub(super) fn views(
    controller: &ComputerUseMcpControllerV1,
    task_id: &str,
    user_id: &str,
    known: &std::collections::BTreeSet<String>,
) -> Vec<ComputerUseTaskSessionViewV1> {
    let Some(runtime) = &controller.runtime else {
        return Vec::new();
    };
    let Ok(records) = runtime.session_journal.latest_events() else {
        return Vec::new();
    };
    records
        .into_iter()
        .filter_map(|event| {
            if event.identity.task_id != task_id
                || event.identity.user_id != user_id
                || event.kind != SessionEventKindV1::SessionInterrupted
                || known.contains(&event.identity.session_id)
            {
                return None;
            }
            let uncertain = matches!(
                event.reconciled_from,
                Some(SessionEventKindV1::ActionIntent | SessionEventKindV1::OutcomeUnknown)
            );
            let disposition = if uncertain {
                format!(
                    "The last action{} has no confirmed outcome. It was not retried; agent control remains fenced.",
                    event.action_sequence.map_or(String::new(), |sequence| format!(" {sequence}"))
                )
            } else {
                "The app restarted and stopped this disposable VM. Agent control remains fenced.".into()
            };
            let inspection_result = controller
                .recovery_notices
                .lock()
                .ok()
                .and_then(|notices| notices.get(&event.identity.session_id).cloned());
            Some(ComputerUseTaskSessionViewV1 {
                session_id: event.identity.session_id,
                task_id: event.identity.task_id,
                user_id: event.identity.user_id,
                agent_role: event.agent_role,
                session_class: event.session_class,
                allowed_apps: event.allowed_apps,
                guest_apps: Vec::new(),
                guest_app_catalog_status: "signed_app_catalog_unavailable".into(),
                app_management_available: false,
                actions_remaining: event.resource_budget.action_limit.saturating_sub(
                    event.action_sequence.unwrap_or_default().min(u32::MAX as u64) as u32,
                ),
                resource_budget: event.resource_budget,
                lifecycle: ComputerLifecycleV1::Interrupted,
                status: if uncertain { Status::Uncertain } else { Status::Interrupted },
                queue_position: None,
                control_owner: ComputerUseControlOwnerV1::Paused,
                control_generation: event.lease_generation,
                active_app: None,
                remaining_ms: event.expires_at_unix_ms.saturating_sub(unix_time_ms()),
                expires_at_unix_ms: event.expires_at_unix_ms,
                frame_revision: 0,
                frame_width: 0,
                frame_height: 0,
                frame_png: None,
                frame_error: true,
                private_input: false,
                control_transition_pending: false,
                control_error: None,
                control_notice: Some(inspection_result.unwrap_or_else(|| disposition.clone())),
                action_log: vec![ComputerUseActionSummaryV1 {
                    actor: if event.action_actor == Some(ComputerActionActorV1::Human) {
                        ComputerUseControlOwnerV1::Human
                    } else {
                        ComputerUseControlOwnerV1::Paused
                    },
                    label: disposition,
                    occurred_at_unix_ms: event.occurred_at_unix_ms,
                }],
            })
        })
        .collect()
}
