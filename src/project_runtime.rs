//! Stack-agnostic project build, test, launch, and readiness contracts.

mod discovery;
mod executor;
mod recipe;
mod store;
mod target;

pub use discovery::{
    DiscoveryContext, DiscoveryEvidence, add_user_target_override, apply_user_recipe_override,
    detect_candidate_targets, discover_manifest,
};
pub use executor::ProjectRuntimeExecutor;
pub use recipe::{ExecutionRecipeV1, NetworkRequirement, RecipeSource};
pub(crate) use recipe::{
    normalize_custom_target_root, validate_custom_artifact_paths, validate_custom_target_root,
};
pub use store::ProjectRuntimeStore;
pub use target::{
    AppSurface, AppTargetV1, ProjectCapabilityManifestV1, ReadinessProbeV1, RuntimeOperation,
    RuntimeReceiptState, RuntimeReceiptV1, TargetCapabilityState,
};

#[cfg(test)]
#[path = "project_runtime/tests.rs"]
mod tests;
