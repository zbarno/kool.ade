//! Project-aware runtime tool discovery.
//!
//! Derives the normalized logical tool requirements of the *approved*
//! recipes attached to a task's active application targets. This replaces
//! filename-heuristic language guessing when a project capability manifest
//! is available: the runtime the implementation session receives reflects
//! what the operator actually approved, not what happened to exist in the
//! repository.

use std::collections::BTreeSet;

use crate::project_runtime::{
    ExecutionRecipeV1, ProjectCapabilityManifestV1, ReadinessProbeV1, RecipeSource,
    is_logical_tool_id,
};

/// Normalized runtime requirements of the approved recipe set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectRuntimeTools {
    /// Canonical logical tool IDs (sorted, de-duplicated).
    pub tools: Vec<String>,
    /// Recipe IDs that contributed to [`Self::tools`] (sorted, de-duplicated).
    pub recipe_ids: Vec<String>,
}

impl ProjectRuntimeTools {
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

/// A recipe counts toward the trusted runtime only when it was either
/// produced by a trusted adapter or explicitly approved by the user with
/// evidence. Detected-but-unvalidated configurations never select tools.
fn recipe_contributes_to_runtime(recipe: &ExecutionRecipeV1) -> bool {
    recipe.is_explicitly_user_approved() || recipe.source == RecipeSource::TrustedAdapter
}

/// Resolves the tools required by the active targets' approved recipes.
///
/// Returns `None` when the manifest has no active target for `task_id` or
/// the approved set yields no logical tools, so callers can fall back to
/// their previous compatibility behavior.
pub fn active_target_recipe_tools(
    manifest: &ProjectCapabilityManifestV1,
    task_id: &str,
) -> Option<ProjectRuntimeTools> {
    let find_recipe = |id: &str| {
        manifest
            .recipes
            .iter()
            .chain(manifest.user_overrides.iter())
            .find(|recipe| recipe.recipe_id == id)
    };

    let active_targets = manifest
        .candidate_targets
        .iter()
        .filter(|target| manifest.is_active_target(task_id, &target.target_id));
    let mut recipe_refs = BTreeSet::new();
    for target in active_targets {
        recipe_refs.extend(target.build_recipe_ref.iter().cloned());
        recipe_refs.extend(target.test_recipe_refs.iter().cloned());
        recipe_refs.extend(target.start_recipe_ref.iter().cloned());
        if let Some(readiness_probe_ref) = target.readiness_probe_ref.as_deref()
            && let Some(ReadinessProbeV1::CustomApprovedProbe { recipe_id }) =
                manifest.readiness_probes.get(readiness_probe_ref)
        {
            recipe_refs.insert(recipe_id.clone());
        }
    }
    let mut recipe_ids = BTreeSet::new();
    let mut tools = BTreeSet::new();
    for id in recipe_refs.into_iter().filter_map(|id| find_recipe(&id)) {
        if !recipe_contributes_to_runtime(id) {
            continue;
        }
        recipe_ids.insert(id.recipe_id.clone());
        for tool in std::iter::once(&id.executable_id)
            .chain(id.prerequisite_tool_ids.iter())
            .filter(|tool| is_logical_tool_id(tool))
        {
            tools.insert(tool.to_owned());
        }
    }
    (!tools.is_empty()).then(|| ProjectRuntimeTools {
        tools: tools.into_iter().collect(),
        recipe_ids: recipe_ids.into_iter().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_runtime::{
        AppSurface, AppTargetV1, NetworkRequirement, TargetCapabilityState,
    };

    fn recipe(id: &str, executable: &str, prereqs: &[&str]) -> ExecutionRecipeV1 {
        recipe_with_source(id, executable, prereqs, RecipeSource::TrustedAdapter)
    }

    fn recipe_with_source(
        id: &str,
        executable: &str,
        prereqs: &[&str],
        source: RecipeSource,
    ) -> ExecutionRecipeV1 {
        let evidence_refs = match source {
            RecipeSource::UserApproved => vec!["user-approval:synthetic-receipt".to_owned()],
            _ => Vec::new(),
        };
        ExecutionRecipeV1 {
            schema_version: 1,
            recipe_id: id.to_owned(),
            source,
            working_directory_relative: ".".to_owned(),
            executable_id: executable.to_owned(),
            argv: Vec::new(),
            allowed_env_keys: Vec::new(),
            allowed_output_paths: Vec::new(),
            timeout_seconds: 120,
            expected_exit_codes: vec![0],
            expected_artifacts: Vec::new(),
            cache_scope: None,
            prerequisite_tool_ids: prereqs.iter().map(|value| (*value).to_owned()).collect(),
            network_requirement: NetworkRequirement::Denied,
            evidence_refs,
        }
    }

    fn target(id: &str, task_id: &str, build: Option<&str>) -> AppTargetV1 {
        AppTargetV1 {
            target_id: id.to_owned(),
            owning_task_id: task_id.to_owned(),
            repository_id: "repository".to_owned(),
            app_surface: AppSurface::Web,
            project_relative_root: ".".to_owned(),
            build_recipe_ref: build.map(ToOwned::to_owned),
            test_recipe_refs: Vec::new(),
            start_recipe_ref: None,
            readiness_probe_ref: None,
            artifact_spec_ref: None,
            fixture_ref: None,
            required_dependencies: Vec::new(),
            supported_driver_ids: Vec::new(),
            port_needs: Vec::new(),
            environment_policy_ref: "environment-policy".to_owned(),
            capability_state: TargetCapabilityState::Ready,
        }
    }

    fn manifest(
        targets: Vec<AppTargetV1>,
        recipes: Vec<ExecutionRecipeV1>,
        overrides: Vec<ExecutionRecipeV1>,
        probes: std::collections::BTreeMap<String, ReadinessProbeV1>,
    ) -> ProjectCapabilityManifestV1 {
        ProjectCapabilityManifestV1 {
            schema_version: 1,
            workspace_id: "workspace".to_owned(),
            repository_id: "repository".to_owned(),
            source_sha: "b".repeat(40),
            detected_languages: Vec::new(),
            frameworks: Vec::new(),
            candidate_targets: targets,
            selected_target_id: None,
            target_capabilities: Vec::new(),
            recipes,
            readiness_probes: probes,
            artifact_specs: std::collections::BTreeMap::new(),
            confidence: 0,
            evidence_refs: Vec::new(),
            user_overrides: overrides,
            revision: 1,
        }
    }

    #[test]
    fn selected_hybrid_target_recipes_select_their_union_of_logical_tools() {
        // One selected target whose approved build and test recipes speak
        // different languages: the projected tool set is their union.
        let mut fullstack = target("fullstack", "task-1", Some("build-service"));
        fullstack.test_recipe_refs.push("test-web".to_owned());
        let mut manifest = manifest(
            vec![fullstack],
            vec![
                recipe("build-service", "cargo", &["rustc"]),
                recipe("test-web", "node", &["npm"]),
            ],
            Vec::new(),
            std::collections::BTreeMap::new(),
        );
        manifest.selected_target_id = Some("fullstack".to_owned());
        let tools = active_target_recipe_tools(&manifest, "task-1")
            .expect("approved recipes must resolve to runtime tools");
        assert_eq!(tools.tools, vec!["cargo", "node", "npm", "rustc"]);
        assert_eq!(tools.recipe_ids, vec!["build-service", "test-web"]);
    }

    #[test]
    fn unresolved_multiple_candidates_degrade_instead_of_guessing() {
        // With two candidates for the task and no operator-selected target,
        // none is authoritative; projecting a runtime from the ambiguous
        // set would silently widen trust, so the caller falls back.
        let manifest = manifest(
            vec![
                target("service", "task-1", Some("build-service")),
                target("frontend", "task-1", Some("build-web")),
            ],
            vec![
                recipe("build-service", "cargo", &["rustc"]),
                recipe("build-web", "node", &["npm"]),
            ],
            Vec::new(),
            std::collections::BTreeMap::new(),
        );
        assert!(active_target_recipe_tools(&manifest, "task-1").is_none());
    }

    #[test]
    fn detected_but_not_approved_recipes_never_select_tools() {
        let manifest = manifest(
            vec![target("service", "task-1", Some("detected-build"))],
            vec![
                recipe_with_source(
                    "detected-build",
                    "python",
                    &[],
                    RecipeSource::ValidatedProjectConfig,
                ),
                recipe("fallback", "cargo", &[]),
            ],
            Vec::new(),
            std::collections::BTreeMap::new(),
        );
        // The only referenced recipe lacks approval, and the unreferenced
        // approved `fallback` recipe must not bleed into the selection.
        assert!(active_target_recipe_tools(&manifest, "task-1").is_none());
    }

    #[test]
    fn inactive_or_other_task_targets_are_excluded() {
        let manifest = manifest(
            vec![
                target("service", "task-1", Some("build-service")),
                target("legacy", "task-2", Some("build-service")),
            ],
            vec![recipe("build-service", "cargo", &[])],
            Vec::new(),
            std::collections::BTreeMap::new(),
        );
        let tools =
            active_target_recipe_tools(&manifest, "task-1").expect("active target must resolve");
        assert_eq!(tools.tools, vec!["cargo"]);
        assert!(active_target_recipe_tools(&manifest, "task-none").is_none());
    }

    #[test]
    fn non_logical_executables_do_not_become_tools() {
        let manifest = manifest(
            vec![target("runner", "task-1", Some("start-binary"))],
            vec![recipe(
                "start-binary",
                "./bin/server",
                &["./scripts/probe.sh", "Bad Package"],
            )],
            Vec::new(),
            std::collections::BTreeMap::new(),
        );
        // Target-relative paths and malformed identifiers are never logical
        // container tool IDs, so the projection stays absent.
        assert!(active_target_recipe_tools(&manifest, "task-1").is_none());
    }

    #[test]
    fn no_active_targets_means_no_projection() {
        let manifest = manifest(
            vec![],
            vec![recipe("build-service", "cargo", &[])],
            Vec::new(),
            std::collections::BTreeMap::new(),
        );
        assert!(active_target_recipe_tools(&manifest, "task-1").is_none());
    }
}
