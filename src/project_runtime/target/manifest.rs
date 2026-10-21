use serde::{Deserialize, Serialize};

use super::{AppSurface, ReadinessProbeV1, TargetCapabilityState};
use crate::project_runtime::recipe::{ExecutionRecipeV1, valid_id, validate_relative_path};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppTargetV1 {
    pub target_id: String,
    pub owning_task_id: String,
    pub repository_id: String,
    pub app_surface: AppSurface,
    pub project_relative_root: String,
    pub build_recipe_ref: Option<String>,
    pub test_recipe_refs: Vec<String>,
    pub start_recipe_ref: Option<String>,
    pub readiness_probe_ref: Option<String>,
    pub artifact_spec_ref: Option<String>,
    pub fixture_ref: Option<String>,
    /// IDs of dependency-broker requests that must be prepared for this target.
    /// QA accepts only matching, task-scoped requests with successful preparation.
    pub required_dependencies: Vec<String>,
    pub supported_driver_ids: Vec<String>,
    pub port_needs: Vec<u16>,
    pub environment_policy_ref: String,
    pub capability_state: TargetCapabilityState,
}

impl AppTargetV1 {
    pub fn validate(&self) -> Result<(), String> {
        for id in [&self.target_id, &self.owning_task_id, &self.repository_id] {
            if !valid_id(id) {
                return Err("application target contains an invalid identifier".into());
            }
        }
        validate_relative_path(&self.project_relative_root)?;
        if self.environment_policy_ref.trim().is_empty()
            || self.environment_policy_ref.len() > 128
            || self.test_recipe_refs.len() > 64
            || self.required_dependencies.len() > 128
            || self.supported_driver_ids.len() > 32
            || self.port_needs.len() > 32
            || self.port_needs.contains(&0)
        {
            return Err("application target policy or bounded lists are invalid".into());
        }
        if self
            .required_dependencies
            .iter()
            .chain(self.supported_driver_ids.iter())
            .any(|id| !valid_dependency_id(id))
        {
            return Err("application target contains an invalid dependency or driver ID".into());
        }
        for reference in self
            .build_recipe_ref
            .iter()
            .chain(self.test_recipe_refs.iter())
            .chain(self.start_recipe_ref.iter())
            .chain(self.readiness_probe_ref.iter())
            .chain(self.artifact_spec_ref.iter())
            .chain(self.fixture_ref.iter())
        {
            if !valid_id(reference) {
                return Err("application target contains an invalid recipe reference".into());
            }
        }
        if self.app_surface == AppSurface::UnsupportedTarget
            && !matches!(
                self.capability_state,
                TargetCapabilityState::UnsupportedPlatform
            )
        {
            return Err("unsupported targets require an unsupported capability state".into());
        }
        Ok(())
    }
}

pub(super) fn valid_dependency_id(value: &str) -> bool {
    let Some((ecosystem, package)) = value.split_once(':') else {
        return valid_id(value);
    };
    if package.is_empty() || package.len() > 214 {
        return false;
    }
    let valid_name = |name: &str| {
        !name.is_empty()
            && name.as_bytes()[0].is_ascii_lowercase()
            && name.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            })
            && !name.ends_with('.')
    };
    match ecosystem {
        "system" => {
            package.as_bytes()[0].is_ascii_lowercase()
                && package.bytes().all(|byte| {
                    byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || matches!(byte, b'+' | b'.' | b'-')
                })
        }
        "cargo" => valid_name(package),
        "npm" => {
            if let Some(scoped) = package.strip_prefix('@') {
                scoped
                    .split_once('/')
                    .is_some_and(|(scope, name)| valid_name(scope) && valid_name(name))
            } else {
                valid_name(package)
            }
        }
        _ => false,
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectCapabilityManifestV1 {
    pub schema_version: u16,
    pub workspace_id: String,
    pub repository_id: String,
    pub source_sha: String,
    pub detected_languages: Vec<String>,
    pub frameworks: Vec<String>,
    pub candidate_targets: Vec<AppTargetV1>,
    /// Explicit app target selected for this task and source revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_target_id: Option<String>,
    pub target_capabilities: Vec<TargetCapabilityState>,
    pub recipes: Vec<ExecutionRecipeV1>,
    pub readiness_probes: std::collections::BTreeMap<String, ReadinessProbeV1>,
    pub artifact_specs: std::collections::BTreeMap<String, Vec<String>>,
    pub confidence: u8,
    pub evidence_refs: Vec<String>,
    pub user_overrides: Vec<ExecutionRecipeV1>,
    pub revision: u64,
}

impl ProjectCapabilityManifestV1 {
    pub fn is_active_target(&self, task_id: &str, target_id: &str) -> bool {
        let Some(target) = self
            .candidate_targets
            .iter()
            .find(|target| target.target_id == target_id)
        else {
            return false;
        };
        if target.owning_task_id != task_id || target.repository_id != self.repository_id {
            return false;
        }
        match self.selected_target_id.as_deref() {
            Some(selected) => selected == target_id,
            None => {
                self.candidate_targets
                    .iter()
                    .filter(|candidate| candidate.owning_task_id == task_id)
                    .count()
                    == 1
            }
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let is_sha = |sha: &str| {
            matches!(sha.len(), 40 | 64) && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
        };
        if self.schema_version != 1
            || !valid_id(&self.workspace_id)
            || !valid_id(&self.repository_id)
            || !is_sha(&self.source_sha)
            || self.confidence > 100
            || self.detected_languages.len() > 64
            || self.frameworks.len() > 128
            || self.candidate_targets.len() > 128
            || self.target_capabilities.len() > 128
            || self.recipes.len() > 256
            || self.user_overrides.len() > 256
            || self.readiness_probes.len() > 128
            || self.artifact_specs.len() > 128
            || self.evidence_refs.len() > 256
            || self.target_capabilities.len() != self.candidate_targets.len()
        {
            return Err("project capability manifest is invalid or exceeds limits".into());
        }
        if self
            .candidate_targets
            .iter()
            .any(|target| target.validate().is_err())
            || self.selected_target_id.as_ref().is_some_and(|selected| {
                !self
                    .candidate_targets
                    .iter()
                    .any(|target| &target.target_id == selected)
            })
            || self
                .recipes
                .iter()
                .chain(self.user_overrides.iter())
                .any(|recipe| recipe.validate().is_err())
            || self
                .readiness_probes
                .iter()
                .any(|(id, probe)| !valid_id(id) || probe.validate().is_err())
            || self.artifact_specs.iter().any(|(id, paths)| {
                !valid_id(id)
                    || paths.is_empty()
                    || paths.len() > 128
                    || paths.iter().any(|path| {
                        validate_relative_path(path).is_err()
                            || path.split('/').any(|part| part == ".git")
                    })
            })
        {
            return Err("project capability manifest contains an invalid target or recipe".into());
        }
        let target_ids = self
            .candidate_targets
            .iter()
            .map(|target| target.target_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        if target_ids.len() != self.candidate_targets.len()
            || self
                .recipes
                .iter()
                .chain(self.user_overrides.iter())
                .map(|recipe| recipe.recipe_id.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != self.recipes.len() + self.user_overrides.len()
        {
            return Err(
                "project capability manifest contains duplicate target or recipe IDs".into(),
            );
        }
        if self
            .candidate_targets
            .iter()
            .zip(&self.target_capabilities)
            .any(|(target, state)| &target.capability_state != state)
        {
            return Err("project target capability summary does not match its targets".into());
        }
        let recipe_ids = self
            .recipes
            .iter()
            .chain(self.user_overrides.iter())
            .map(|recipe| recipe.recipe_id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let probe_ids = self
            .readiness_probes
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        let artifact_ids = self
            .artifact_specs
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        for target in &self.candidate_targets {
            if target
                .build_recipe_ref
                .iter()
                .chain(target.test_recipe_refs.iter())
                .chain(target.start_recipe_ref.iter())
                .any(|reference| !recipe_ids.contains(reference.as_str()))
                || target
                    .readiness_probe_ref
                    .as_ref()
                    .is_some_and(|reference| !probe_ids.contains(reference.as_str()))
                || target
                    .artifact_spec_ref
                    .as_ref()
                    .is_some_and(|reference| !artifact_ids.contains(reference.as_str()))
            {
                return Err("application target references missing runtime metadata".into());
            }
        }
        Ok(())
    }
}
