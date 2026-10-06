use crate::harness::{AiHarness, ClaudeHarness, CodexHarness, PiHarness};

pub(crate) fn configured_harness(
    override_harness: &mut Option<Box<dyn AiHarness>>,
) -> Box<dyn AiHarness> {
    configured_harness_for(override_harness, None)
}

pub(crate) fn configured_harness_for(
    override_harness: &mut Option<Box<dyn AiHarness>>,
    work_type: Option<&str>,
) -> Box<dyn AiHarness> {
    if let Some(harness) = override_harness.take() {
        return harness;
    }
    let settings = crate::persistence::harness_settings::load().0;
    let legacy = (std::env::var(crate::harness::CODEX_HARNESS_ENV).as_deref() == Ok("codex"))
        .then_some("codex");
    let harness = routed_harness(&settings, work_type, legacy);
    if work_type == Some(crate::persistence::harness_settings::IMPLEMENTATION) {
        let qa = routed_harness(
            &settings,
            Some(crate::persistence::harness_settings::QA),
            legacy,
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
) -> Box<dyn AiHarness> {
    let (selected, route) = route_selection(settings, work_type, legacy);
    let harness = resolve(selected);
    let harness_id = route
        .map(|route| route.harness.as_str())
        .or(selected)
        .unwrap_or("pi");
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
) -> (
    Option<&'a str>,
    Option<&'a crate::persistence::harness_settings::WorkRoute>,
) {
    let route = work_type.and_then(|category| settings.work_routes.get(category));
    let selected = route
        .map(|route| route.harness.as_str())
        .or(settings.default_harness.as_deref())
        .or(legacy);
    (selected, route)
}

struct RoutedHarness {
    inner: Box<dyn AiHarness>,
    harness_id: String,
    model: Option<String>,
    default_model_hint: Option<String>,
}

impl AiHarness for RoutedHarness {
    fn label(&self) -> String {
        format!(
            "{} / {}",
            self.inner.label(),
            self.model.as_deref().map(str::to_owned).unwrap_or_else(|| {
                self.default_model_hint.as_ref().map_or_else(
                    || format!("CLI default (model not reported for {})", self.harness_id),
                    |model| format!("CLI default (last discovered: {model})"),
                )
            })
        )
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        self.inner.check_available()
    }
    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.inner
            .execute_with_model(request, self.model.as_deref())
    }
    fn execute_with_model(
        &self,
        request: &crate::harness::PlanningRequest,
        model: Option<&str>,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.inner
            .execute_with_model(request, model.or(self.model.as_deref()))
    }
    fn plan_retrieval(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<Option<crate::harness::RetrievalPlan>, crate::error::AppError> {
        self.inner.plan_retrieval(request)
    }
}

struct ImplementationQaHarness {
    implementation: Box<dyn AiHarness>,
    qa: Box<dyn AiHarness>,
}

impl AiHarness for ImplementationQaHarness {
    fn label(&self) -> String {
        format!("{} · QA: {}", self.implementation.label(), self.qa.label())
    }
    fn check_available(&self) -> Result<String, crate::error::AppError> {
        let implementation = self.implementation.check_available()?;
        self.qa.check_available()?;
        Ok(implementation)
    }
    fn execute(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        if request.telemetry_phase.as_deref() == Some("qa_verification") {
            self.qa.execute(request)
        } else {
            self.implementation.execute(request)
        }
    }
    fn plan_retrieval(
        &self,
        request: &crate::harness::PlanningRequest,
    ) -> Result<Option<crate::harness::RetrievalPlan>, crate::error::AppError> {
        self.implementation.plan_retrieval(request)
    }
}

fn resolve(selected: Option<&str>) -> Box<dyn AiHarness> {
    match selected {
        None | Some("pi") => Box::new(PiHarness),
        Some("codex") => Box::new(CodexHarness),
        Some("claude") => Box::new(ClaudeHarness),
        Some(id) => Box::new(UnavailableHarness(id.to_owned())),
    }
}

struct UnavailableHarness(String);

impl AiHarness for UnavailableHarness {
    fn label(&self) -> String {
        format!("{} (unavailable)", self.0)
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Err(self.error())
    }

    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        Err(self.error())
    }
}

impl UnavailableHarness {
    fn error(&self) -> crate::error::AppError {
        crate::error::AppError::Other(if self.0.starts_with("The selected model ") {
            self.0.clone()
        } else {
            format!(
                "The saved coding harness route '{}' is unavailable in this Kool.ad/e build. Open Coding tool configuration to choose an available harness.",
                self.0
            )
        })
    }
}

#[cfg(test)]
#[path = "harness_selection/tests.rs"]
mod tests;
