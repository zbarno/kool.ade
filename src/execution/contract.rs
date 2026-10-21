mod events;
mod ids;
mod readiness;
mod request;
mod result;
mod target_preference;

pub use events::{
    ArtifactManifestEntryV1, EventAcceptance, EventPageRequestV1, EventStreamValidator,
    ExecutionEventKindV1, ExecutionEventV1, ExecutionLifecycleV1, ResourceMetricsV1,
};
pub use ids::{
    ExecutionTarget, TargetLocation, WorkspaceRef, is_valid_sha, validate_dependency_request_id,
    validate_identifier,
};
pub use readiness::{
    HarnessCapabilitySnapshotV1, HarnessRole, ModeReadinessV1, ProviderReadinessV1, ReadinessState,
    RoleReadinessV1,
};
pub use request::{
    DependencyCacheEcosystemV1, DependencyCacheRefV1, ExecutionModeV1, ExecutionRequestV1,
    ExecutionWireMessageV1, MAX_EXECUTION_DURATION_MS, MAX_SCRIPT_BYTES, NetworkAccessV1,
    ResourceBudgetV1, ensure_wire_compatible,
};
pub use result::ExecutionResultV1;
pub use target_preference::TargetPreference;

pub const EXECUTION_WIRE_VERSION_V1: u16 = 1;
