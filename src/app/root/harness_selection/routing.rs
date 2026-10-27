use super::role_router::role_for_work_category;
use super::wrappers::{ImplementationQaHarness, RoutedHarness, UnavailableHarness};
use super::{role_router, route_readiness::has_current_authenticated_snapshot};
use crate::harness::{
    AiHarness, AntigravityHarness, ClaudeHarness, CodexHarness, CopilotHarness, OpenCodeHarness,
    PiHarness,
};

pub(crate) fn configured_harness_for_project(
    override_harness: &mut Option<Box<dyn AiHarness>>,
    work_type: Option<&str>,
    project_routes: &std::collections::BTreeMap<
        String,
        crate::persistence::harness_settings::WorkRoute,
    >,
) -> Box<dyn AiHarness> {
    configured_harness_for_project_task(
        override_harness,
        work_type,
        project_routes,
        &Default::default(),
    )
}

#[cfg(test)]
pub(crate) fn configured_harness_for_task(
    override_harness: &mut Option<Box<dyn AiHarness>>,
    work_type: Option<&str>,
    task_routes: &std::collections::BTreeMap<
        String,
        crate::persistence::harness_settings::WorkRoute,
    >,
) -> Box<dyn AiHarness> {
    configured_harness_for_project_task(
        override_harness,
        work_type,
        &std::collections::BTreeMap::new(),
        task_routes,
    )
}

pub(crate) fn configured_harness_for_project_task(
    override_harness: &mut Option<Box<dyn AiHarness>>,
    work_type: Option<&str>,
    project_routes: &std::collections::BTreeMap<
        String,
        crate::persistence::harness_settings::WorkRoute,
    >,
    task_routes: &std::collections::BTreeMap<
        String,
        crate::persistence::harness_settings::WorkRoute,
    >,
) -> Box<dyn AiHarness> {
    if let Some(harness) = override_harness.take() {
        return harness;
    }
    let settings = crate::persistence::harness_settings::load().0;
    if work_type == Some(crate::persistence::harness_settings::MANAGER)
        && task_routes.contains_key(crate::persistence::harness_settings::MANAGER)
    {
        return Box::new(UnavailableHarness::guidance(
            "Kool.ad/e Manager is application-routed and cannot be overridden by a task".into(),
        ));
    }
    let legacy = (std::env::var(crate::harness::CODEX_HARNESS_ENV).as_deref() == Ok("codex"))
        .then_some("codex");
    // Uncategoried turns use the general planning route before the legacy
    // implementation route fallback.
    let task_route = work_type
        .and_then(|category| task_routes.get(category))
        .or_else(|| {
            work_type
                .is_none()
                .then(|| {
                    task_routes
                        .get(crate::persistence::harness_settings::PLANNING)
                        .or_else(|| {
                            task_routes.get(crate::persistence::harness_settings::IMPLEMENTATION)
                        })
                })
                .flatten()
        });
    let harness = routed_harness(&settings, project_routes, work_type, legacy, task_route);
    if work_type == Some(crate::persistence::harness_settings::IMPLEMENTATION) {
        let qa = routed_harness(
            &settings,
            project_routes,
            Some(crate::persistence::harness_settings::QA),
            legacy,
            task_routes.get(crate::persistence::harness_settings::QA),
        );
        let repair = (task_routes.contains_key(crate::persistence::harness_settings::REPAIR)
            || project_routes.contains_key(crate::persistence::harness_settings::REPAIR)
            || settings
                .work_routes
                .contains_key(crate::persistence::harness_settings::REPAIR))
        .then(|| {
            routed_harness(
                &settings,
                project_routes,
                Some(crate::persistence::harness_settings::REPAIR),
                legacy,
                task_routes.get(crate::persistence::harness_settings::REPAIR),
            )
        });
        Box::new(ImplementationQaHarness {
            implementation: harness,
            qa,
            repair,
        })
    } else {
        harness
    }
}

pub(super) fn routed_harness<'a>(
    settings: &'a crate::persistence::harness_settings::HarnessSettings,
    project_routes: &'a std::collections::BTreeMap<
        String,
        crate::persistence::harness_settings::WorkRoute,
    >,
    work_type: Option<&'a str>,
    legacy: Option<&'a str>,
    task_route: Option<&'a crate::persistence::harness_settings::WorkRoute>,
) -> Box<dyn AiHarness> {
    let (selected, route) =
        route_selection(settings, project_routes, work_type, legacy, task_route);
    let Some(harness_id) = route.map(|route| route.harness.as_str()).or(selected) else {
        return Box::new(UnavailableHarness::guidance(
            "No CLI is selected. Choose a default in Coding tools; Kool.ad/e will not silently route work to a provider.".into(),
        ));
    };
    let harness = resolve(selected);
    let detected = settings.discovered.get(harness_id);
    let now = crate::persistence::harness_settings::current_unix_ms();
    if let Some(reason) =
        crate::harness::execution_security::task_execution_block_reason(harness_id)
    {
        return Box::new(UnavailableHarness::guidance(reason.into()));
    }
    let discovery_fresh = detected.is_some_and(|entry| entry.is_fresh_at(now));
    let authenticated =
        detected.is_some_and(|entry| has_current_authenticated_snapshot(harness_id, entry, now));
    if !discovery_fresh {
        return Box::new(UnavailableHarness::guidance(format!(
            "Discovery for {harness_id} is stale. Refresh Coding tools before starting this task; no fallback will run it."
        )));
    }
    if !detected.is_some_and(|entry| entry.ready) {
        return Box::new(UnavailableHarness::guidance(format!(
            "Harness {harness_id} is currently unavailable. Review its Coding tools diagnostic; no fallback will run this task."
        )));
    }
    if !authenticated {
        return Box::new(UnavailableHarness::guidance(format!(
            "Authentication for harness {harness_id} is unconfirmed or unavailable; no current authenticated capability snapshot exists. Refresh Coding tools before starting this task; no fallback will run it."
        )));
    }
    let category = work_type.unwrap_or(crate::persistence::harness_settings::PLANNING);
    let role = role_for_work_category(category)
        .unwrap_or(crate::execution::contract::HarnessRole::PlanningAnalyst);
    if !detected.is_some_and(|entry| entry.supports_role_or_context_only_planning_at(role, now)) {
        return Box::new(UnavailableHarness::guidance(format!(
            "Harness {harness_id} has no current capability proof for the {category:?} role; refresh Coding tools and review its provider diagnostics."
        )));
    }
    let implementation_route =
        work_type == Some(crate::persistence::harness_settings::IMPLEMENTATION);
    if implementation_route {
        if !detected.is_some_and(|entry| entry.implementation_available) {
            return Box::new(UnavailableHarness::guidance(format!(
                "Harness {harness_id} is not currently available for implementation inside Kool.ad/e's application-owned Docker execution session. Refresh Coding tools or select a supported tool."
            )));
        }
        if !crate::harness::implementation_route_available(harness_id) {
            return Box::new(UnavailableHarness::guidance(format!(
                "Harness {harness_id} has no supported application-owned implementation route. Select a supported tool before starting the task."
            )));
        }
    }
    // Route selection establishes provider and model identity. Whether this
    // operation needs repository tools is determined by PlanningRequest.mode
    // at execution time; a context-only role must remain usable without a
    // repository execution target.
    let explicit_model = route
        .and_then(|route| route.model.as_deref())
        .filter(|model| !model.trim().is_empty());
    if let Some(model) = explicit_model
        && !detected.is_some_and(|entry| entry.has_verified_model_at(harness_id, model, now))
    {
        return Box::new(UnavailableHarness::guidance(format!(
            "The selected model '{model}' is not in the refreshed {harness_id} CLI model catalog. Open Coding tools, refresh discovery, and choose an available model."
        )));
    }
    let discovered_default_model =
        detected.and_then(|entry| entry.verified_default_model_at(harness_id, now));
    if explicit_model.is_none() && discovered_default_model.is_none() {
        return Box::new(UnavailableHarness::guidance(format!(
            "No verified model is selected for {harness_id}. Refresh its model catalog or choose an available model in Coding tools; this task will not run with an unreported CLI default."
        )));
    }
    let snapshot = detected.and_then(|entry| entry.capability_snapshot.as_ref());
    // Pin the fresh provider default so provenance matches the model passed to
    // the CLI. Preserve the difference between an explicit route and that
    // effective selection.
    let requested_model = explicit_model.map(str::to_owned);
    let selected_model = explicit_model.or(discovered_default_model);
    Box::new(RoutedHarness {
        inner: harness,
        harness_id: harness_id.to_owned(),
        model: selected_model.map(str::to_owned),
        provenance: crate::harness::HarnessRouteProvenance {
            provider_id: harness_id.to_owned(),
            role,
            work_category: Some(category.to_owned()),
            selected_model: selected_model.map(str::to_owned),
            requested_model,
            capability_probe_id: snapshot.map(|snapshot| snapshot.probe_id.clone()),
            capability_generation: snapshot.map(|snapshot| snapshot.probe_sequence),
        },
    })
}

pub(super) fn route_selection<'a>(
    settings: &'a crate::persistence::harness_settings::HarnessSettings,
    project_routes: &'a std::collections::BTreeMap<
        String,
        crate::persistence::harness_settings::WorkRoute,
    >,
    work_type: Option<&'a str>,
    legacy: Option<&'a str>,
    task_route: Option<&'a crate::persistence::harness_settings::WorkRoute>,
) -> (
    Option<&'a str>,
    Option<&'a crate::persistence::harness_settings::WorkRoute>,
) {
    let selected = role_router::resolve(role_router::RouteSources {
        category: work_type,
        task_override: task_route,
        project_routes,
        global_routes: &settings.work_routes,
        default_harness: settings.default_harness.as_deref(),
        legacy_harness: legacy,
    });
    (selected.harness, selected.route)
}

pub(super) fn resolve(selected: Option<&str>) -> Box<dyn AiHarness> {
    match selected {
        None => Box::new(UnavailableHarness::guidance(
            "No CLI is selected. Choose a default in Coding tools; Kool.ad/e will not silently route work to a provider.".into(),
        )),
        Some("pi") => Box::new(PiHarness),
        Some("codex") => Box::new(CodexHarness),
        Some("claude") => Box::new(ClaudeHarness),
        Some("copilot") => Box::new(CopilotHarness),
        Some("opencode") => Box::new(OpenCodeHarness),
        Some("antigravity") => Box::new(AntigravityHarness),
        Some(id) => Box::new(UnavailableHarness::unknown_saved_route(id.to_owned())),
    }
}
