use crate::harness::{AiHarness, PlanningRequest};

mod unavailable;
pub(super) use unavailable::UnavailableHarness;

pub(super) struct RoutedHarness {
    pub(super) inner: Box<dyn AiHarness>,
    pub(super) harness_id: String,
    pub(super) model: Option<String>,
    pub(super) provenance: crate::harness::HarnessRouteProvenance,
}

impl AiHarness for RoutedHarness {
    fn capability_refresh_binding(&self) -> Option<crate::harness::CapabilityRefreshBindingV1> {
        Some(crate::harness::CapabilityRefreshBindingV1 {
            provider_id: self.provenance.provider_id.clone(),
            role: self.provenance.role,
            work_category: self.provenance.work_category.clone()?,
            selected_model: self.provenance.selected_model.clone()?,
            requested_model: self.provenance.requested_model.clone(),
            observed_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        })
    }

    fn supports_context_retrieval(&self) -> bool {
        self.inner.supports_context_retrieval()
    }

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
        if let Some(reason) = crate::harness::execution_security::profile_block_reason(
            &self.harness_id,
            request.mode.profile(),
        ) {
            return Err(crate::error::AppError::Other(reason.into()));
        }
        self.validate_capability_evidence()?;
        self.inner
            .plan_retrieval_with_model(request, self.model.as_deref())
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
        crate::harness::require_application_implementation_boundary(&self.harness_id, request)?;
        self.validate_capability_evidence()
    }

    fn validate_capability_evidence(&self) -> Result<(), crate::error::AppError> {
        let expired = self
            .provenance
            .capability_expires_at_unix_ms
            .is_none_or(|expires| {
                crate::persistence::harness_settings::current_unix_ms() > expires
            });
        let expected = self
            .provenance
            .cli_version
            .as_deref()
            .filter(|version| !version.trim().is_empty());
        if expired || expected.is_none() {
            crate::app::dialogs::request_cli_auto_refresh();
            return Err(crate::error::AppError::Other(format!(
                "CLI {} version or role-profile evidence expired or changed. Kool.ad/e is refreshing eligibility; retry when verification completes.",
                self.harness_id
            )));
        }
        let profile = self.provenance.role.harness_mode().profile();
        let authentication_mode = self.provenance.authentication_mode;
        let expected_profile = authentication_mode.map(|mode| {
            crate::execution::contract::profile_evidence_id_for_snapshot(
                &self.harness_id,
                expected,
                mode,
                profile,
            )
        });
        let profile_is_current = authentication_mode.is_some_and(|mode| {
            mode != crate::execution::contract::AuthenticationModeV1::Unknown
                && mode == crate::execution::contract::current_authentication_mode(&self.harness_id)
                && expected_profile.as_deref().is_some_and(|id| {
                    !id.is_empty() && self.provenance.profile_evidence_id.as_deref() == Some(id)
                })
        });
        if !profile_is_current {
            crate::app::dialogs::request_cli_auto_refresh();
            return Err(crate::error::AppError::Other(format!(
                "CLI {} authentication or role-profile evidence changed. Kool.ad/e is refreshing eligibility; retry when verification completes.",
                self.harness_id
            )));
        }
        let actual = match self.inner.check_available() {
            Ok(version) => version,
            Err(error) => {
                crate::app::dialogs::request_cli_auto_refresh();
                return Err(error);
            }
        };
        if expected != Some(actual.trim()) {
            crate::app::dialogs::request_cli_auto_refresh();
            return Err(crate::error::AppError::Other(format!(
                "CLI {} version or role-profile evidence expired or changed. Kool.ad/e is refreshing eligibility; retry when verification completes.",
                self.harness_id
            )));
        }
        Ok(())
    }
}

pub(super) struct ImplementationQaHarness {
    pub(super) implementation: Box<dyn AiHarness>,
    pub(super) qa: Box<dyn AiHarness>,
    pub(super) repair: Option<Box<dyn AiHarness>>,
}

impl AiHarness for ImplementationQaHarness {
    fn capability_refresh_binding(&self) -> Option<crate::harness::CapabilityRefreshBindingV1> {
        self.implementation.capability_refresh_binding()
    }

    fn route_blocker(&self) -> Option<String> {
        self.implementation.route_blocker()
    }

    fn supports_context_retrieval(&self) -> bool {
        self.implementation.supports_context_retrieval()
    }

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

    fn plan_retrieval_with_model(
        &self,
        request: &PlanningRequest,
        model: Option<&str>,
    ) -> Result<Option<crate::harness::RetrievalPlan>, crate::error::AppError> {
        self.implementation
            .plan_retrieval_with_model(request, model)
    }
}
