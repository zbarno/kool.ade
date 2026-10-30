//! Explicit machine-local helper route; never a repository planning route.
use super::{routing, wrappers::UnavailableHarness};
use crate::{
    error::AppError,
    harness::{AiHarness, HarnessOutcome, HarnessRouteProvenance, PlanningRequest},
};

pub(crate) fn configured_bootstrap_harness(
    override_harness: &mut Option<Box<dyn AiHarness>>,
) -> Box<dyn AiHarness> {
    if let Some(harness) = override_harness.take() {
        return harness;
    }
    let settings = crate::persistence::harness_settings::load().0;
    let unavailable = || {
        Box::new(UnavailableHarness::guidance(
        "Choose and verify a setup helper in Coding Tools before asking Man.ager. No other provider will be selected automatically.".into(),
    )) as Box<dyn AiHarness>
    };
    let Some(id) = settings.bootstrap_provider.as_deref() else {
        return unavailable();
    };
    let Some(entry) = settings.discovered.get(id) else {
        return unavailable();
    };
    let now = crate::persistence::harness_settings::current_unix_ms();
    if !entry.ready
        || entry.configuration_required
        || crate::harness::execution_security::task_execution_block_reason(id).is_some()
        || !entry.has_current_authenticated_capability(id, now)
    {
        return unavailable();
    }
    let Some(snapshot) = entry
        .capability_snapshot
        .as_ref()
        .filter(|snapshot| snapshot.supports_context_only_help())
    else {
        return unavailable();
    };
    let Some(model) = entry.verified_default_model_at(id, now) else {
        return unavailable();
    };
    Box::new(BootstrapHarness {
        inner: routing::resolve(Some(id)),
        model: model.into(),
        provenance: HarnessRouteProvenance {
            provider_id: id.into(),
            role: crate::execution::contract::HarnessRole::PlanningAnalyst,
            work_category: Some(crate::persistence::harness_settings::PLANNING.into()),
            selected_model: Some(model.into()),
            requested_model: None,
            capability_probe_id: Some(snapshot.probe_id.clone()),
            capability_generation: Some(snapshot.probe_sequence),
        },
    })
}

struct BootstrapHarness {
    inner: Box<dyn AiHarness>,
    model: String,
    provenance: HarnessRouteProvenance,
}

fn is_bootstrap_request(request: &PlanningRequest) -> bool {
    request.mode == crate::harness::ExecutionMode::ReadOnlyAnalysis
        && request.execution_workspace.is_none()
        && request.task_id.is_none()
        && request.runtime_config_source.is_none()
        && request.destination_sha.is_none()
        && request.authorization_verifier.is_none()
        && request.review_identity.is_none()
        && request
            .prompt_manifest
            .as_ref()
            .is_some_and(|manifest| manifest.role_version == "bootstrap_conversation/v2")
}

impl AiHarness for BootstrapHarness {
    fn label(&self) -> String {
        self.inner.label()
    }
    fn check_available(&self) -> Result<String, AppError> {
        self.inner.check_available()
    }
    fn route_provenance(&self, request: &PlanningRequest) -> Option<HarnessRouteProvenance> {
        is_bootstrap_request(request).then(|| self.provenance.clone())
    }
    fn execute(&self, request: &PlanningRequest) -> Result<HarnessOutcome, AppError> {
        if !is_bootstrap_request(request) {
            return Err(AppError::Other(
                "Setup help cannot execute repository or non-bootstrap requests.".into(),
            ));
        }
        self.inner.execute_with_model(request, Some(&self.model))
    }
}

#[cfg(test)]
mod tests;
