use crate::harness::{AiHarness, PlanningRequest};

pub(super) struct RoutedHarness {
    pub(super) inner: Box<dyn AiHarness>,
    pub(super) harness_id: String,
    pub(super) model: Option<String>,
    pub(super) provenance: crate::harness::HarnessRouteProvenance,
}

impl AiHarness for RoutedHarness {
    fn route_provenance(
        &self,
        _request: &PlanningRequest,
    ) -> Option<crate::harness::HarnessRouteProvenance> {
        Some(self.provenance.clone())
    }
    fn label(&self) -> String {
        format!(
            "{} / {}",
            self.inner.label(),
            self.model.as_deref().map(str::to_owned).unwrap_or_else(|| {
                format!("CLI default (model not reported for {})", self.harness_id)
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
            .map_err(|error| self.normalize_failure(request, error))
    }

    fn execute_with_model(
        &self,
        request: &PlanningRequest,
        model: Option<&str>,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        self.ensure_mediated_implementation(request)?;
        self.inner
            .execute_with_model(request, model.or(self.model.as_deref()))
            .map_err(|error| self.normalize_failure(request, error))
    }

    fn plan_retrieval(
        &self,
        request: &PlanningRequest,
    ) -> Result<Option<crate::harness::RetrievalPlan>, crate::error::AppError> {
        self.inner
            .plan_retrieval(request)
            .map_err(|error| self.normalize_failure(request, error))
    }
}

impl RoutedHarness {
    fn normalize_failure(
        &self,
        request: &PlanningRequest,
        error: crate::error::AppError,
    ) -> crate::error::AppError {
        crate::harness::normalize_provider_error(
            &self.harness_id,
            self.provenance.role,
            request,
            error,
        )
    }

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
    pub(super) repair: Option<Box<dyn AiHarness>>,
}

impl AiHarness for ImplementationQaHarness {
    fn route_provenance(
        &self,
        request: &PlanningRequest,
    ) -> Option<crate::harness::HarnessRouteProvenance> {
        if request.telemetry_phase.as_deref() == Some("qa_verification") {
            self.qa.route_provenance(request)
        } else if request.telemetry_phase.as_deref() == Some("repair") {
            self.repair
                .as_deref()
                .unwrap_or(self.implementation.as_ref())
                .route_provenance(request)
        } else {
            self.implementation.route_provenance(request)
        }
    }
    fn label(&self) -> String {
        format!("{} · QA: {}", self.implementation.label(), self.qa.label())
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        let implementation = self.implementation.check_available()?;
        self.qa.check_available()?;
        if let Some(repair) = &self.repair {
            repair.check_available()?;
        }
        Ok(implementation)
    }

    fn execute(
        &self,
        request: &PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        if matches!(
            request.telemetry_phase.as_deref(),
            Some("qa_verification" | "runtime_qa_scenario_synthesis")
        ) {
            let mut read_only = request.clone();
            read_only.mode = crate::harness::ExecutionMode::ReadOnlyAnalysis;
            self.qa.execute(&read_only)
        } else if request.telemetry_phase.as_deref() == Some("repair") {
            self.repair
                .as_deref()
                .unwrap_or(self.implementation.as_ref())
                .execute(request)
        } else {
            self.implementation.execute(request)
        }
    }

    fn execute_with_model(
        &self,
        request: &PlanningRequest,
        model: Option<&str>,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        if request.telemetry_phase.as_deref() == Some("qa_verification") {
            let mut read_only = request.clone();
            read_only.mode = crate::harness::ExecutionMode::ReadOnlyAnalysis;
            self.qa.execute_with_model(&read_only, model)
        } else if request.telemetry_phase.as_deref() == Some("repair") {
            self.repair
                .as_deref()
                .unwrap_or(self.implementation.as_ref())
                .execute_with_model(request, model)
        } else {
            self.implementation.execute_with_model(request, model)
        }
    }

    fn plan_retrieval(
        &self,
        request: &PlanningRequest,
    ) -> Result<Option<crate::harness::RetrievalPlan>, crate::error::AppError> {
        self.implementation.plan_retrieval(request)
    }
}

pub(super) struct UnavailableHarness {
    pub(super) message: String,
    /// A saved route identifier this build cannot resolve is framed as a
    /// "saved route unavailable" error; anything else is user-facing guidance
    /// and must render verbatim.
    pub(super) saved_route: bool,
}

impl AiHarness for UnavailableHarness {
    fn label(&self) -> String {
        format!("{} (unavailable)", self.message)
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
        crate::error::AppError::Other(if self.saved_route {
            format!(
                "The saved coding harness route '{}' is unavailable in this Kool.ad/e build. Open Coding tool configuration to choose an available harness.",
                self.message
            )
        } else {
            self.message.clone()
        })
    }
}

impl UnavailableHarness {
    /// Operator guidance rendered verbatim.
    pub(super) fn guidance(message: String) -> Self {
        Self {
            message,
            saved_route: false,
        }
    }

    /// A persisted route identifier this build cannot resolve.
    pub(super) fn unknown_saved_route(route_id: String) -> Self {
        Self {
            message: route_id,
            saved_route: true,
        }
    }
}
