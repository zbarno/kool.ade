mod bootstrap;
pub(crate) use bootstrap::configured_bootstrap_harness;
mod cli_notice;
mod role_router;
mod route_readiness;
mod routing;
mod wrappers;
#[cfg(test)]
use crate::harness::AiHarness;
pub(crate) use cli_notice::cli_routing_notice;
pub(crate) use role_router::role_for_work_category;
use route_readiness::has_current_authenticated_snapshot;
#[cfg(test)]
pub(crate) use routing::configured_harness_for_task;
pub(crate) use routing::{configured_harness_for_project, configured_harness_for_project_task};
#[cfg(test)]
use wrappers::{ImplementationQaHarness, RoutedHarness, UnavailableHarness};

#[cfg(test)]
fn route_selection<'a>(
    settings: &'a crate::persistence::harness_settings::HarnessSettings,
    work_type: Option<&'a str>,
    legacy: Option<&'a str>,
    task_route: Option<&'a crate::persistence::harness_settings::WorkRoute>,
) -> (
    Option<&'a str>,
    Option<&'a crate::persistence::harness_settings::WorkRoute>,
) {
    routing::route_selection(
        settings,
        empty_project_routes(),
        work_type,
        legacy,
        task_route,
    )
}

#[cfg(test)]
fn empty_project_routes()
-> &'static std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute> {
    static ROUTES: std::sync::OnceLock<
        std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
    > = std::sync::OnceLock::new();
    ROUTES.get_or_init(std::collections::BTreeMap::new)
}

#[cfg(test)]
fn routed_harness(
    settings: &crate::persistence::harness_settings::HarnessSettings,
    work_type: Option<&str>,
    legacy: Option<&str>,
    task_route: Option<&crate::persistence::harness_settings::WorkRoute>,
) -> Box<dyn crate::harness::AiHarness> {
    routing::routed_harness(
        settings,
        empty_project_routes(),
        work_type,
        legacy,
        task_route,
    )
}

#[cfg(test)]
fn resolve(selected: Option<&str>) -> Box<dyn crate::harness::AiHarness> {
    routing::resolve(selected)
}

pub(crate) fn validate_task_routes(
    settings: &crate::persistence::harness_settings::HarnessSettings,
    routes: &std::collections::BTreeMap<String, crate::persistence::harness_settings::WorkRoute>,
) -> Result<(), String> {
    for (category, route) in routes {
        if category == crate::persistence::harness_settings::MANAGER {
            return Err(
                "Kool.ad/e Manager is application-routed and cannot be overridden by a task".into(),
            );
        }
        if !crate::persistence::harness_settings::is_task_routable_category(category) {
            return Err(format!(
                "Task work category '{category}' cannot be overridden"
            ));
        }
        let Some(detected) = settings.discovered.get(&route.harness) else {
            return Err(format!(
                "Harness '{}' has not been discovered for task routing",
                route.harness
            ));
        };
        if !detected.is_fresh_at(crate::persistence::harness_settings::current_unix_ms()) {
            crate::app::dialogs::request_cli_auto_refresh();
            return Err(format!(
                "Harness '{}' is being reverified automatically before task routing",
                route.harness
            ));
        }
        if !detected.ready {
            return Err(format!(
                "Harness '{}' is unavailable for task routing",
                route.harness
            ));
        }
        if !has_current_authenticated_snapshot(
            &route.harness,
            detected,
            crate::persistence::harness_settings::current_unix_ms(),
        ) {
            return Err(format!(
                "Authentication for harness '{}' is unconfirmed or unavailable; no current authenticated capability snapshot exists. Refresh Coding tools and verify provider login before task routing",
                route.harness
            ));
        }
        let role = role_for_work_category(category).ok_or_else(|| {
            format!("Task work category '{category}' has no supported harness role")
        })?;
        if !detected.supports_role_or_context_only_planning_at(
            role,
            crate::persistence::harness_settings::current_unix_ms(),
        ) {
            return Err(format!(
                "Harness '{}' has no current capability proof for the {category} role; refresh Coding tools and review its provider diagnostics",
                route.harness
            ));
        }
        if *category == crate::persistence::harness_settings::IMPLEMENTATION
            && (!detected.implementation_available
                || !crate::harness::implementation_route_available(&route.harness))
        {
            return Err(format!(
                "Harness '{}' cannot run inside Kool.ad/e's application-owned Docker execution session. Select a supported tool before starting the task.",
                route.harness
            ));
        }
        let selected_model = route
            .model
            .as_deref()
            .filter(|model| !model.trim().is_empty());
        if let Some(model) = selected_model
            && !detected.has_verified_model_at(
                &route.harness,
                model,
                crate::persistence::harness_settings::current_unix_ms(),
            )
        {
            return Err(format!(
                "Model '{model}' is unavailable for harness '{}'",
                route.harness
            ));
        }
        if selected_model.is_none()
            && detected
                .verified_default_model_at(
                    &route.harness,
                    crate::persistence::harness_settings::current_unix_ms(),
                )
                .is_none()
        {
            return Err(format!(
                "Harness '{}' has no verified default model; choose a model from its fresh catalog",
                route.harness
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "harness_selection/tests.rs"]
mod tests;
