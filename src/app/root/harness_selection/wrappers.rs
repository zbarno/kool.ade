use crate::harness::{AiHarness, PlanningRequest};

pub(super) struct RoutedHarness {
    pub(super) inner: Box<dyn AiHarness>,
    pub(super) harness_id: String,
    pub(super) model: Option<String>,
    pub(super) default_model_hint: Option<String>,
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
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.ensure_mediated_implementation(request)?;
        self.inner
            .execute_with_model(request, self.model.as_deref())
    }

    fn execute_with_model(
        &self,
        request: &PlanningRequest,
        model: Option<&str>,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.ensure_mediated_implementation(request)?;
        self.inner
            .execute_with_model(request, model.or(self.model.as_deref()))
    }

    fn plan_retrieval(
        &self,
        request: &PlanningRequest,
    ) -> Result<Option<crate::harness::RetrievalPlan>, crate::error::AppError> {
        self.inner.plan_retrieval(request)
    }
}

impl RoutedHarness {
    fn ensure_mediated_implementation(
        &self,
        request: &PlanningRequest,
    ) -> Result<(), crate::error::AppError> {
        crate::harness::require_application_implementation_boundary(&self.harness_id, request)
    }
}

pub(super) struct ImplementationQaHarness {
    pub(super) implementation: Box<dyn AiHarness>,
    pub(super) qa: Box<dyn AiHarness>,
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
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        if request.telemetry_phase.as_deref() == Some("qa_verification") {
            let mut read_only = request.clone();
            read_only.mode = crate::harness::ExecutionMode::ReadOnlyAnalysis;
            self.qa.execute(&read_only)
        } else {
            self.implementation.execute(request)
        }
    }

    fn plan_retrieval(
        &self,
        request: &PlanningRequest,
    ) -> Result<Option<crate::harness::RetrievalPlan>, crate::error::AppError> {
        self.implementation.plan_retrieval(request)
    }
}

pub(super) struct UnavailableHarness(pub(super) String);

impl AiHarness for UnavailableHarness {
    fn label(&self) -> String {
        format!("{} (unavailable)", self.0)
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Err(self.error())
    }

    fn execute(
        &self,
        _request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        Err(self.error())
    }
}

impl UnavailableHarness {
    fn error(&self) -> crate::error::AppError {
        crate::error::AppError::Other(
            if self.0.starts_with("The selected model ")
                || self.0.starts_with("Implementation is unavailable")
                || self.0.starts_with("Repository access is unavailable")
            {
                self.0.clone()
            } else {
                format!(
                    "The saved coding harness route '{}' is unavailable in this Kool.ad/e build. Open Coding tool configuration to choose an available harness.",
                    self.0
                )
            },
        )
    }
}
