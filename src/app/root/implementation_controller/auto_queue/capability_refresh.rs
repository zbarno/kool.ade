use crate::app::session::Project;
use std::collections::BTreeSet;

pub(in crate::app::root) fn recoverable_tickets(
    project: &mut Project,
    cancelled: &BTreeSet<String>,
) -> Vec<String> {
    if !project.queue.auto_build || project.queue.recovery_paused {
        return Vec::new();
    }
    let mut recovery = project
        .queue
        .recoverable_tickets(&project.task_documents)
        .into_iter()
        .filter(|ticket| !cancelled.contains(ticket))
        .filter(|ticket| {
            project.queue.blocked.get(ticket).is_none_or(|failure| {
                failure.recovery
                    != crate::core::implementation::RecoveryDisposition::AwaitCapabilityRefresh
            })
        })
        .collect::<Vec<_>>();
    let now = crate::persistence::harness_settings::current_unix_ms();
    let exhausted = project
        .queue
        .blocked
        .iter()
        .filter(|(ticket, failure)| {
            failure.recovery
                == crate::core::implementation::RecoveryDisposition::AwaitCapabilityRefresh
                && !cancelled.contains(*ticket)
                && project
                    .queue
                    .recovery_attempts
                    .get(*ticket)
                    .copied()
                    .unwrap_or(0)
                    >= 2
        })
        .map(|(ticket, _)| ticket.clone())
        .collect::<Vec<_>>();
    let mut backoff_changed = false;
    for ticket in exhausted {
        if let Some(failure) = project.queue.blocked.get_mut(&ticket) {
            failure.recovery = crate::core::implementation::RecoveryDisposition::UserAction;
            failure.retry_after_unix_ms = None;
            failure.message = "The selected harness capability did not remain valid through two automatic resume attempts. Review the current route and explicitly resume the task.".into();
            backoff_changed = true;
        }
    }
    let refresh_waiting = project
        .queue
        .blocked
        .iter()
        .filter(|(ticket, failure)| {
            failure.recovery
                == crate::core::implementation::RecoveryDisposition::AwaitCapabilityRefresh
                && !cancelled.contains(*ticket)
                && project
                    .queue
                    .recovery_attempts
                    .get(*ticket)
                    .copied()
                    .unwrap_or(0)
                    < 2
        })
        .map(|(ticket, _)| ticket.clone())
        .collect::<Vec<_>>();
    for ticket in refresh_waiting {
        let due = project
            .queue
            .blocked
            .get(&ticket)
            .and_then(|failure| failure.retry_after_unix_ms)
            .is_none_or(|deadline| now >= deadline);
        if !due {
            continue;
        }
        let Some(failure) = project.queue.blocked.get(&ticket) else {
            continue;
        };
        let Some(binding) = failure.capability_refresh_binding.as_ref() else {
            continue;
        };
        let task_routes = project
            .task_documents
            .iter()
            .find(|doc| doc.path == ticket)
            .and_then(|doc| doc.metadata.as_ref())
            .map(|metadata| metadata.routing_overrides.clone())
            .unwrap_or_default();
        match crate::app::root::harness_selection::capability_refresh_route_state(
            binding,
            &project.state.harness_routes.routes,
            &task_routes,
        ) {
            crate::app::root::harness_selection::CapabilityRefreshRouteState::Ready => {
                recovery.push(ticket)
            }
            crate::app::root::harness_selection::CapabilityRefreshRouteState::Pending => {
                if let Some(failure) = project.queue.blocked.get_mut(&ticket) {
                    failure.retry_after_unix_ms = Some(now.saturating_add(5_000));
                    backoff_changed = true;
                }
            }
            state => {
                if let Some(failure) = project.queue.blocked.get_mut(&ticket) {
                    failure.recovery = crate::core::implementation::RecoveryDisposition::UserAction;
                    failure.retry_after_unix_ms = None;
                    failure.message = match state {
                        crate::app::root::harness_selection::CapabilityRefreshRouteState::RouteChanged => "The selected harness route or model changed while eligibility was refreshing. Review and explicitly resume the task to authorize the current route.".into(),
                        _ => "The selected harness did not produce fresh valid capability proof. Review Coding tools and explicitly resume the task after resolving the reported provider issue.".into(),
                    };
                    backoff_changed = true;
                }
            }
        }
    }
    if backoff_changed && let Err(error) = project.queue.save(&project.state.planning_store.root) {
        project.queue.last_error =
            format!("Cannot persist capability refresh retry time: {error:#}");
        return Vec::new();
    }
    recovery
}

#[cfg(test)]
#[path = "capability_refresh/tests.rs"]
mod tests;
