use crate::harness::{
    AiHarness, AntigravityHarness, ClaudeHarness, CodexHarness, CopilotHarness, OpenCodeHarness,
    PiHarness,
};
mod wrappers;
use wrappers::{ImplementationQaHarness, RoutedHarness, UnavailableHarness};

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
        if !matches!(
            category.as_str(),
            crate::persistence::harness_settings::IMPLEMENTATION
                | crate::persistence::harness_settings::QA
                | crate::persistence::harness_settings::DOCUMENTATION
        ) {
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
        if !detected.ready {
            return Err(format!(
                "Harness '{}' is unavailable for task routing",
                route.harness
            ));
        }
        if let Some(model) = route.model.as_deref()
            && !detected.models.iter().any(|available| available == model)
        {
            return Err(format!(
                "Model '{model}' is unavailable for harness '{}'",
                route.harness
            ));
        }
    }
    Ok(())
}

pub(crate) fn configured_harness(
    override_harness: &mut Option<Box<dyn AiHarness>>,
) -> Box<dyn AiHarness> {
    configured_harness_for(override_harness, None)
}

pub(crate) fn configured_harness_for(
    override_harness: &mut Option<Box<dyn AiHarness>>,
    work_type: Option<&str>,
) -> Box<dyn AiHarness> {
    configured_harness_for_task(override_harness, work_type, &Default::default())
}

pub(crate) fn configured_harness_for_task(
    override_harness: &mut Option<Box<dyn AiHarness>>,
    work_type: Option<&str>,
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
        return Box::new(UnavailableHarness(
            "Kool.ad/e Manager is application-routed and cannot be overridden by a task".into(),
        ));
    }
    let legacy = (std::env::var(crate::harness::CODEX_HARNESS_ENV).as_deref() == Ok("codex"))
        .then_some("codex");
    let task_route = work_type.and_then(|category| task_routes.get(category));
    let harness = routed_harness(&settings, work_type, legacy, task_route);
    if work_type == Some(crate::persistence::harness_settings::IMPLEMENTATION) {
        let qa = routed_harness(
            &settings,
            Some(crate::persistence::harness_settings::QA),
            legacy,
            task_routes.get(crate::persistence::harness_settings::QA),
        );
        Box::new(ImplementationQaHarness {
            implementation: harness,
            qa,
        })
    } else {
        harness
    }
}

fn routed_harness(
    settings: &crate::persistence::harness_settings::HarnessSettings,
    work_type: Option<&str>,
    legacy: Option<&str>,
    task_route: Option<&crate::persistence::harness_settings::WorkRoute>,
) -> Box<dyn AiHarness> {
    let (selected, route) = route_selection(settings, work_type, legacy, task_route);
    let harness = resolve(selected);
    let harness_id = route
        .map(|route| route.harness.as_str())
        .or(selected)
        .unwrap_or("pi");
    if work_type == Some(crate::persistence::harness_settings::IMPLEMENTATION) && harness_id != "pi"
    {
        return Box::new(UnavailableHarness(
            "Implementation is available only with Pi until this harness uses Kool.ad/e's dependency authorization broker. Select Pi in Coding tools; other harnesses remain available for planning and QA.".into(),
        ));
    }
    let explicit_model = route
        .and_then(|route| route.model.as_deref())
        .filter(|model| !model.trim().is_empty());
    if let Some(model) = explicit_model
        && !settings
            .discovered
            .get(harness_id)
            .is_some_and(|detected| detected.models.iter().any(|available| available == model))
    {
        return Box::new(UnavailableHarness(format!(
            "The selected model '{model}' is not in the refreshed {harness_id} CLI model catalog. Open Coding tools, refresh discovery, and choose an available model."
        )));
    }
    let default_model_hint = settings
        .discovered
        .get(harness_id)
        .and_then(|detected| detected.default_model.clone());
    Box::new(RoutedHarness {
        inner: harness,
        harness_id: harness_id.to_owned(),
        model: explicit_model.map(str::to_owned),
        default_model_hint,
    })
}

fn route_selection<'a>(
    settings: &'a crate::persistence::harness_settings::HarnessSettings,
    work_type: Option<&str>,
    legacy: Option<&'a str>,
    task_route: Option<&'a crate::persistence::harness_settings::WorkRoute>,
) -> (
    Option<&'a str>,
    Option<&'a crate::persistence::harness_settings::WorkRoute>,
) {
    let route =
        task_route.or_else(|| work_type.and_then(|category| settings.work_routes.get(category)));
    let selected = route
        .map(|route| route.harness.as_str())
        .or(settings.default_harness.as_deref())
        .or(legacy);
    (selected, route)
}

fn resolve(selected: Option<&str>) -> Box<dyn AiHarness> {
    match selected {
        None | Some("pi") => Box::new(PiHarness),
        Some("codex") => Box::new(CodexHarness),
        Some("claude") => Box::new(ClaudeHarness),
        Some("copilot") => Box::new(CopilotHarness),
        Some("opencode") => Box::new(OpenCodeHarness),
        Some("antigravity") => Box::new(AntigravityHarness),
        Some(id) => Box::new(UnavailableHarness(id.to_owned())),
    }
}

#[cfg(test)]
#[path = "harness_selection/tests.rs"]
mod tests;
