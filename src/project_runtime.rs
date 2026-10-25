//! Stack-agnostic project build, test, launch, and readiness contracts.

mod adapter;
mod discovery;
mod executor;
mod recipe;
mod source;
mod store;
mod target;

pub(crate) use adapter::trusted_adapter_template;
pub(crate) use discovery::target_id_for_evidence_path;
pub use discovery::{
    DiscoveryContext, DiscoveryEvidence, add_user_target_override, apply_user_recipe_override,
    detect_candidate_targets, discover_manifest,
};
pub use executor::ProjectRuntimeExecutor;
pub use recipe::{ExecutionRecipeV1, NetworkRequirement, RecipeSource};
pub(crate) use recipe::{
    is_logical_tool_id, is_protected_environment_key, is_target_relative_executable_path,
    normalize_custom_target_root, validate_custom_artifact_paths,
    validate_custom_target_artifact_paths, validate_custom_target_root,
};
pub(crate) use source::verify_source_checkout;
pub use store::ProjectRuntimeStore;
pub use target::{
    AppSurface, AppTargetV1, ProjectCapabilityManifestV1, ReadinessProbeV1, RuntimeOperation,
    RuntimeReceiptState, RuntimeReceiptV1, TargetCapabilityState,
};

#[cfg(test)]
#[path = "project_runtime/tests.rs"]
mod tests;
