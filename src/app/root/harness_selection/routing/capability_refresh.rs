use super::*;

pub(super) fn stale_binding(
    settings: &crate::persistence::harness_settings::HarnessSettings,
    harness_id: &str,
    role: crate::execution::contract::HarnessRole,
    category: &str,
    requested_model: Option<&str>,
) -> Option<crate::harness::CapabilityRefreshBindingV1> {
    let selected_model = requested_model.or_else(|| {
        settings
            .discovered
            .get(harness_id)
            .and_then(|entry| entry.default_model.as_deref())
            .filter(|model| !model.trim().is_empty())
    })?;
    Some(crate::harness::CapabilityRefreshBindingV1 {
        provider_id: harness_id.to_owned(),
        role,
        work_category: category.to_owned(),
        selected_model: selected_model.to_owned(),
        requested_model: requested_model.map(str::to_owned),
        observed_at_unix_ms: settings
            .discovered
            .get(harness_id)
            .map_or(0, |entry| entry.checked_at_unix_ms),
    })
}

pub(super) fn refreshing_harness(
    detected: Option<&crate::persistence::harness_settings::DetectedHarness>,
    harness_id: &str,
    now: u64,
    binding: Option<crate::harness::CapabilityRefreshBindingV1>,
) -> Option<UnavailableHarness> {
    let stale_profile = detected
        .and_then(|entry| entry.capability_snapshot.as_ref())
        .is_none_or(|snapshot| !snapshot.profile_evidence_is_current());
    let stale_discovery = !detected.is_some_and(|entry| entry.is_fresh_at(now));
    if !stale_profile && !stale_discovery {
        return None;
    }
    crate::app::dialogs::request_cli_auto_refresh();
    let message = if stale_profile {
        format!(
            "CLI {harness_id} role profile or authentication source changed. Kool.ad/e is refreshing its eligibility evidence; retry when verification completes."
        )
    } else {
        format!(
            "CLI {harness_id} is being reverified automatically. Retry shortly if it does not resume; unverified execution remains blocked."
        )
    };
    Some(match binding.filter(|binding| binding.is_valid()) {
        Some(binding) => UnavailableHarness::refreshing(message, binding),
        None => UnavailableHarness::guidance(message),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CapabilityRefreshRouteState {
    Ready,
    Pending,
    Failed,
    RouteChanged,
}

pub(crate) fn capability_refresh_route_state(
    binding: &crate::harness::CapabilityRefreshBindingV1,
    project_routes: &std::collections::BTreeMap<
        String,
        crate::persistence::harness_settings::WorkRoute,
    >,
    task_routes: &std::collections::BTreeMap<
        String,
        crate::persistence::harness_settings::WorkRoute,
    >,
) -> CapabilityRefreshRouteState {
    if !binding.is_valid() {
        return CapabilityRefreshRouteState::Failed;
    }
    let settings = crate::persistence::harness_settings::load().0;
    let route = task_routes.get(&binding.work_category);
    let legacy = (std::env::var(crate::harness::CODEX_HARNESS_ENV).as_deref() == Ok("codex"))
        .then_some("codex");
    let harness = super::routed_harness(
        &settings,
        project_routes,
        Some(&binding.work_category),
        legacy,
        route,
    );
    let current_binding = harness.capability_refresh_binding();
    if harness.route_blocker().is_none()
        && current_binding
            .as_ref()
            .is_some_and(|current| current.same_route(binding))
    {
        return CapabilityRefreshRouteState::Ready;
    }
    if current_binding
        .as_ref()
        .is_some_and(|current| !current.same_route(binding))
    {
        return CapabilityRefreshRouteState::RouteChanged;
    }
    let refreshed = settings
        .discovered
        .get(&binding.provider_id)
        .is_some_and(|entry| entry.checked_at_unix_ms > binding.observed_at_unix_ms);
    if refreshed || !crate::app::dialogs::cli_auto_refresh_in_flight() {
        CapabilityRefreshRouteState::Failed
    } else {
        CapabilityRefreshRouteState::Pending
    }
}
