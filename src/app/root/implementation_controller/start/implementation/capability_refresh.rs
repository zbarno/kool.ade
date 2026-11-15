use crate::{app::session::Project, core::implementation::Failure, harness::AiHarness};

pub(super) fn pending_override(harness: Option<&dyn AiHarness>) -> Option<Failure> {
    let harness = harness?;
    Some(Failure::awaiting_capability_refresh(
        harness.route_blocker()?,
        harness.capability_refresh_binding()?,
    ))
}

pub(super) fn pending_route(harness: &dyn AiHarness) -> Option<Failure> {
    Some(Failure::awaiting_capability_refresh(
        harness.route_blocker()?,
        harness.capability_refresh_binding()?,
    ))
}

pub(super) fn select_for_ticket(
    project: &mut Project,
    ticket: &str,
    cached: &mut Option<Box<dyn AiHarness>>,
) -> Option<Box<dyn AiHarness>> {
    if let Some(failure) = pending_override(cached.as_deref()) {
        persist(project, ticket, failure);
        return None;
    }
    let routes = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
        .and_then(|document| document.metadata.as_ref())
        .map(|metadata| metadata.routing_overrides.clone())
        .unwrap_or_default();
    let harness = crate::app::root::harness_selection::configured_harness_for_project_task(
        cached,
        Some(crate::persistence::harness_settings::IMPLEMENTATION),
        &project.state.harness_routes.routes,
        &routes,
    );
    if let Some(failure) = pending_route(harness.as_ref()) {
        persist(project, ticket, failure);
        return None;
    }
    Some(harness)
}

pub(super) fn persist(project: &mut Project, ticket: &str, failure: Failure) {
    project.queue.last_error = failure.message.clone();
    project.queue.in_flight.remove(ticket);
    project.queue.blocked.insert(ticket.to_owned(), failure);
    if let Err(error) = project.queue.save(&project.state.planning_store.root) {
        project.queue.last_error = format!("Cannot persist capability refresh blocker: {error:#}");
    }
    if project.active_implementations.is_empty() {
        project.queue_lock = None;
    }
}
