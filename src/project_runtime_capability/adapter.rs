//! Adapter from the persisted runtime contract to the generic capability resolver.
use super::*;
use crate::project_runtime::{AppSurface, ProjectCapabilityManifestV1, TargetCapabilityState};

/// Capability facts must come from the selected Docker image, dependency broker,
/// and installed runtime-driver registry. Host PATH and user environment are not inputs.
pub struct ApprovedCapabilityInventory<'a> {
    pub manifest: &'a ProjectCapabilityManifestV1,
    pub available_tool_ids: &'a BTreeSet<String>,
    pub available_dependency_ids: &'a BTreeSet<String>,
    pub available_driver_ids: &'a BTreeSet<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestTargetAssessment {
    pub capability_state: TargetCapabilityState,
    pub report: TargetCapabilityAssessment,
}

impl CapabilityInventory for ApprovedCapabilityInventory<'_> {
    fn recipe(&self, recipe_id: &str) -> Option<RecipeCapabilityView<'_>> {
        // Explicit user overrides take precedence over discovered project metadata.
        let recipe = self
            .manifest
            .user_overrides
            .iter()
            .chain(self.manifest.recipes.iter())
            .find(|recipe| recipe.recipe_id == recipe_id)?;
        Some(RecipeCapabilityView {
            authorized: recipe.is_explicitly_user_approved(),
            prerequisite_tool_ids: &recipe.prerequisite_tool_ids,
        })
    }

    fn tool_available(&self, id: &str) -> bool {
        self.available_tool_ids.contains(id)
    }
    fn dependency_available(&self, id: &str) -> bool {
        self.available_dependency_ids.contains(id)
    }
    fn driver_available(&self, id: &str) -> bool {
        self.available_driver_ids.contains(id)
    }
}

impl TargetCapabilityView for crate::project_runtime::AppTargetV1 {
    fn target_id(&self) -> &str {
        &self.target_id
    }

    fn surface_id(&self) -> &str {
        match self.app_surface {
            AppSurface::Web => "web",
            AppSurface::LinuxDesktop => "linux_desktop",
            AppSurface::HeadlessService => "headless_service",
            AppSurface::UnsupportedTarget => "unsupported_target",
        }
    }

    fn recipe_ids(&self) -> Vec<&str> {
        self.build_recipe_ref
            .iter()
            .map(String::as_str)
            .chain(self.test_recipe_refs.iter().map(String::as_str))
            .chain(self.start_recipe_ref.iter().map(String::as_str))
            .collect()
    }

    fn has_build_recipe(&self) -> bool {
        self.build_recipe_ref.is_some()
    }
    fn has_test_recipe(&self) -> bool {
        !self.test_recipe_refs.is_empty()
    }
    fn has_start_recipe(&self) -> bool {
        self.start_recipe_ref.is_some()
    }
    fn has_readiness_probe(&self) -> bool {
        self.readiness_probe_ref.is_some()
    }
    fn required_dependency_ids(&self) -> &[String] {
        &self.required_dependencies
    }
    fn supported_driver_ids(&self) -> &[String] {
        &self.supported_driver_ids
    }
}

/// Resolves a task's explicit target; it never guesses from language or picks a
/// different app in a monorepo. Stale/foreign task assignments are rejected.
pub fn assess_manifest_target(
    manifest: &ProjectCapabilityManifestV1,
    target_id: &str,
    task_id: &str,
    inventory: &ApprovedCapabilityInventory<'_>,
) -> anyhow::Result<ManifestTargetAssessment> {
    manifest.validate().map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        inventory.manifest.workspace_id == manifest.workspace_id
            && inventory.manifest.repository_id == manifest.repository_id
            && inventory.manifest.source_sha == manifest.source_sha,
        "runtime inventory does not match the selected manifest revision"
    );
    let mut matches = manifest
        .candidate_targets
        .iter()
        .filter(|target| target.target_id == target_id);
    let target = matches
        .next()
        .ok_or_else(|| anyhow::anyhow!("selected runtime target is missing"))?;
    anyhow::ensure!(
        matches.next().is_none(),
        "runtime manifest contains duplicate target IDs"
    );
    anyhow::ensure!(
        target.owning_task_id == task_id,
        "runtime target is not assigned to this task"
    );
    anyhow::ensure!(
        target.repository_id == manifest.repository_id,
        "runtime target belongs to another repository"
    );
    let report = assess_target(target, inventory);
    let state = if report
        .gaps
        .iter()
        .any(|gap| matches!(gap, CapabilityGap::UnsupportedSurface(_)))
    {
        TargetCapabilityState::UnsupportedPlatform
    } else if report
        .gaps
        .iter()
        .any(|gap| matches!(gap, CapabilityGap::UnauthorizedRecipe(_)))
    {
        TargetCapabilityState::UnauthorizedRecipe
    } else if report
        .gaps
        .iter()
        .any(|gap| matches!(gap, CapabilityGap::MissingToolchain(_)))
    {
        TargetCapabilityState::MissingToolchain
    } else if report
        .gaps
        .iter()
        .any(|gap| matches!(gap, CapabilityGap::MissingDependency(_)))
    {
        TargetCapabilityState::MissingDependencies
    } else if report
        .gaps
        .iter()
        .any(|gap| matches!(gap, CapabilityGap::MissingDriver(_)))
    {
        TargetCapabilityState::DriverUnavailable
    } else if !report.gaps.is_empty() {
        TargetCapabilityState::ReadyButUntested
    } else {
        TargetCapabilityState::Ready
    };
    if state == TargetCapabilityState::UnsupportedPlatform {
        anyhow::ensure!(
            target.capability_state == TargetCapabilityState::UnsupportedPlatform,
            "manifest target claims support for an unsupported surface"
        );
    }
    Ok(ManifestTargetAssessment {
        capability_state: state,
        report,
    })
}

#[cfg(test)]
#[path = "adapter_tests.rs"]
mod tests;
