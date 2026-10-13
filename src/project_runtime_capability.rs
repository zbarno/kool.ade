//! Resolves runtime prerequisites and QA applicability without depending on a
//! particular language, framework, manifest schema, or execution backend.
use std::collections::BTreeSet;

mod adapter;
pub use adapter::{ApprovedCapabilityInventory, ManifestTargetAssessment, assess_manifest_target};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QaApplicability {
    Visual,
    ServiceEvidence,
    StaticReviewOnly,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapabilityGap {
    MissingRecipe(String),
    UnauthorizedRecipe(String),
    MissingToolchain(String),
    MissingDependency(String),
    MissingDriver(String),
    MissingStartRecipe,
    MissingReadinessProbe,
    MissingBuildRecipe,
    MissingTestRecipe,
    UnsupportedSurface(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetCapabilityAssessment {
    pub target_id: String,
    pub qa_applicability: QaApplicability,
    pub selected_driver_id: Option<String>,
    pub gaps: Vec<CapabilityGap>,
}

impl CapabilityGap {
    pub fn remediation(&self) -> String {
        match self {
            Self::MissingRecipe(id) => format!("Add or select a validated recipe for `{id}`."),
            Self::UnauthorizedRecipe(id) => format!("Approve recipe `{id}` explicitly or replace it with a trusted adapter."),
            Self::MissingToolchain(id) => format!("Request approved toolchain `{id}` through the dependency broker."),
            Self::MissingDependency(id) => format!("Authorize or provide dependency `{id}` through the dependency broker."),
            Self::MissingDriver(id) => format!("Enable a compatible application-surface driver such as `{id}`."),
            Self::MissingStartRecipe => "Add an approved target start recipe before runtime QA.".into(),
            Self::MissingReadinessProbe => "Add an approved readiness probe before claiming the application is ready.".into(),
            Self::MissingBuildRecipe => "Select or approve a build recipe; implementation and static review can continue meanwhile.".into(),
            Self::MissingTestRecipe => "Select or approve a test recipe; do not report runtime tests as passed.".into(),
            Self::UnsupportedSurface(surface) => format!("Surface `{surface}` has no supported runtime driver here; continue planning and static review without claiming runtime QA."),
        }
    }
}

/// Read-only view of a project-specific target. Adapters map existing typed
/// target contracts into this trait rather than copying their stored schema.
pub trait TargetCapabilityView {
    fn target_id(&self) -> &str;
    fn surface_id(&self) -> &str;
    fn recipe_ids(&self) -> Vec<&str>;
    fn has_build_recipe(&self) -> bool;
    fn has_test_recipe(&self) -> bool;
    fn has_start_recipe(&self) -> bool;
    fn has_readiness_probe(&self) -> bool;
    fn required_dependency_ids(&self) -> &[String];
    fn supported_driver_ids(&self) -> &[String];
}

pub struct RecipeCapabilityView<'a> {
    pub authorized: bool,
    pub prerequisite_tool_ids: &'a [String],
}

/// Supplies observed capabilities from the selected execution environment.
/// This is normally derived from its approved image and brokers, never host PATH.
pub trait CapabilityInventory {
    fn recipe(&self, recipe_id: &str) -> Option<RecipeCapabilityView<'_>>;
    fn tool_available(&self, tool_id: &str) -> bool;
    fn dependency_available(&self, dependency_id: &str) -> bool;
    fn driver_available(&self, driver_id: &str) -> bool;
}

pub fn assess_target<T: TargetCapabilityView, I: CapabilityInventory>(
    target: &T,
    inventory: &I,
) -> TargetCapabilityAssessment {
    let surface = target.surface_id();
    let unsupported = surface == "unsupported_target";
    let visual = matches!(surface, "web" | "linux_desktop");
    let service = surface == "headless_service";
    let mut gaps = Vec::new();
    let mut missing_tools = BTreeSet::new();
    let mut missing_dependencies = BTreeSet::new();

    for recipe_id in target.recipe_ids() {
        match inventory.recipe(recipe_id) {
            None => gaps.push(CapabilityGap::MissingRecipe(recipe_id.into())),
            Some(recipe) if !recipe.authorized => {
                gaps.push(CapabilityGap::UnauthorizedRecipe(recipe_id.into()));
            }
            Some(recipe) => {
                for tool_id in recipe.prerequisite_tool_ids {
                    if !inventory.tool_available(tool_id) {
                        missing_tools.insert(tool_id.clone());
                    }
                }
            }
        }
    }
    for dependency in target.required_dependency_ids() {
        if !inventory.dependency_available(dependency) {
            missing_dependencies.insert(dependency.clone());
        }
    }
    gaps.extend(
        missing_tools
            .into_iter()
            .map(CapabilityGap::MissingToolchain),
    );
    gaps.extend(
        missing_dependencies
            .into_iter()
            .map(CapabilityGap::MissingDependency),
    );

    let selected_driver_id = if visual {
        target
            .supported_driver_ids()
            .iter()
            .find(|id| inventory.driver_available(id))
            .cloned()
    } else {
        None
    };
    if visual && selected_driver_id.is_none() {
        gaps.push(CapabilityGap::MissingDriver(
            target
                .supported_driver_ids()
                .first()
                .cloned()
                .unwrap_or_else(|| "compatible visual driver".into()),
        ));
    }
    if unsupported || (!visual && !service) {
        gaps.push(CapabilityGap::UnsupportedSurface(surface.into()));
    }
    if visual && !target.has_start_recipe() {
        gaps.push(CapabilityGap::MissingStartRecipe);
    }
    if visual && !target.has_readiness_probe() {
        gaps.push(CapabilityGap::MissingReadinessProbe);
    }
    if !target.has_build_recipe() {
        gaps.push(CapabilityGap::MissingBuildRecipe);
    }
    if !target.has_test_recipe() {
        gaps.push(CapabilityGap::MissingTestRecipe);
    }

    let hard_blocker = unsupported || (!visual && !service);
    let qa_applicability = if hard_blocker {
        QaApplicability::Unsupported
    } else if visual
        && selected_driver_id.is_some()
        && target.has_start_recipe()
        && target.has_readiness_probe()
    {
        QaApplicability::Visual
    } else if service && target.has_test_recipe() {
        QaApplicability::ServiceEvidence
    } else {
        QaApplicability::StaticReviewOnly
    };
    TargetCapabilityAssessment {
        target_id: target.target_id().into(),
        qa_applicability,
        selected_driver_id,
        gaps,
    }
}

#[cfg(test)]
mod tests;
