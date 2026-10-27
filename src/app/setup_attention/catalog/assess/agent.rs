use crate::execution::contract::ReadinessState;
use crate::harness::discovery::{ModelCatalogStatus, ProbeResult, ProbeStatus};

pub(super) fn authenticated_context(probe: &ProbeResult, generation: u64) -> bool {
    probe.status == ProbeStatus::Available
        && probe.run_generation == generation
        && probe.binary_path.is_some()
        && probe.version.is_some()
        && probe.authentication == ReadinessState::Ready
        && probe.execution_capability.safe_for_planning == ReadinessState::Ready
        && probe.model_catalog_status == ModelCatalogStatus::Fresh
        && (!probe.models.is_empty() || probe.default_model.is_some())
}
