//! Deterministic, non-executable plans for project-specific Docker toolchains.
//!
//! This is deliberately a *proposal*, not an image-build authorization. The
//! session must continue to reject a proposed composite until an app-owned
//! builder verifies and registers its immutable image ID and tool inventory.

use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use super::{
    ImageLock, PrerequisiteCode, PrerequisiteFailure, RuntimeProfile, SelectedImage, failure,
    task_runtime_profile_order,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ProjectRuntimeImagePlan {
    /// A single currently approved image already satisfies this recipe.
    Existing(SelectedImage),
    /// All tools have approved component sources, but the combined image must
    /// be constructed, verified and explicitly authorized before execution.
    Composition(CompositionCandidate),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CompositionCandidate {
    pub platform: String,
    pub foundation: SelectedImage,
    pub components: Vec<SelectedImage>,
    pub required_tools: Vec<String>,
    /// Stable *recipe* identity only. Never treat it as an OCI image digest,
    /// proof of package installation or an authorization token.
    pub recipe_sha256: String,
}

impl ImageLock {
    /// Plan the minimum set of approved *source images* needed to satisfy the
    /// logical tools in one approved recipe. Project metadata and agent output
    /// are not permitted to supply image references.
    ///
    /// This does not build, pull, inspect or authorize a runtime. The caller
    /// must retain its source/task/recipe approval and network-denial gates.
    pub(crate) fn plan_project_runtime(
        &self,
        daemon_arch: &str,
        required_tools: &[String],
    ) -> Result<ProjectRuntimeImagePlan, PrerequisiteFailure> {
        if required_tools.iter().any(|tool| tool.is_empty()) {
            return Err(failure(
                PrerequisiteCode::ProfileUnsupported,
                "An approved runtime recipe contains an empty tool ID",
                false,
            ));
        }
        if let Ok(image) = self.select_with_tools(
            task_runtime_profile_order(required_tools),
            daemon_arch,
            required_tools,
        ) {
            return Ok(ProjectRuntimeImagePlan::Existing(image));
        }

        // An exact, architecture-matched general image is always the trusted
        // foundation. A missing foundation is a hard error, not a fallback to
        // a task-provided or mutable base.
        let foundation = self.select(RuntimeProfile::General, daemon_arch)?;
        let mut uncovered = required_tools
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut components = Vec::new();

        // Greedily use the source that covers the most remaining capabilities.
        // Ties are broken by the fixed, reviewed profile ordering and never by
        // the order in which the task happened to list its tools.
        let candidates = [
            RuntimeProfile::Rust,
            RuntimeProfile::Node,
            RuntimeProfile::Python,
            RuntimeProfile::Go,
        ]
        .into_iter()
        .filter_map(|profile| self.select(profile, daemon_arch).ok())
        .collect::<Vec<_>>();

        while !uncovered.is_empty() {
            let candidate = candidates
                .iter()
                .filter(|candidate| {
                    !components
                        .iter()
                        .any(|selected: &SelectedImage| selected.profile == candidate.profile)
                })
                .max_by_key(|candidate| {
                    uncovered
                        .iter()
                        .filter(|tool| candidate.provides_tool(tool))
                        .count()
                });
            let Some(candidate) = candidate else {
                return Err(missing_tool_failure(&uncovered));
            };
            if !uncovered.iter().any(|tool| candidate.provides_tool(tool)) {
                return Err(missing_tool_failure(&uncovered));
            }
            uncovered.retain(|tool| !candidate.provides_tool(tool));
            components.push(candidate.clone());
        }
        components.sort_by_key(|component| component.profile);

        let canonical_tools = required_tools
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        // Include exact source digests AND advertised tool versions. The
        // builder's future recipe/security policy version must change this
        // schema marker when those semantics change.
        let identity = serde_json::to_vec(&(
            "koolade-runtime-composition-recipe-v1",
            &foundation.platform,
            &foundation.digest,
            &foundation.tool_versions,
            &components
                .iter()
                .map(|image| (&image.profile, &image.digest, &image.tool_versions))
                .collect::<Vec<_>>(),
            &canonical_tools,
        ))
        .map_err(|_| {
            failure(
                PrerequisiteCode::ImageUnavailable,
                "The project runtime plan could not be encoded",
                false,
            )
        })?;
        let recipe_sha256 = format!("sha256:{:x}", Sha256::digest(identity));
        Ok(ProjectRuntimeImagePlan::Composition(CompositionCandidate {
            platform: foundation.platform.clone(),
            foundation,
            components,
            required_tools: canonical_tools,
            recipe_sha256,
        }))
    }
}

fn missing_tool_failure(uncovered: &BTreeSet<&str>) -> PrerequisiteFailure {
    failure(
        PrerequisiteCode::ProfileUnsupported,
        &format!(
            "No approved, pinned runtime component provides required tool(s): {}",
            uncovered.iter().copied().collect::<Vec<_>>().join(", ")
        ),
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(arch: &str, tools: &[&str]) -> Result<ProjectRuntimeImagePlan, PrerequisiteFailure> {
        ImageLock::bundled()?.plan_project_runtime(
            arch,
            &tools
                .iter()
                .map(|tool| (*tool).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    fn composite(arch: &str, tools: &[&str]) -> CompositionCandidate {
        match plan(arch, tools).unwrap() {
            ProjectRuntimeImagePlan::Composition(candidate) => candidate,
            other => panic!("expected a composition, got {other:?}"),
        }
    }

    #[test]
    fn a_single_language_still_uses_the_existing_approved_image() {
        match plan("amd64", &["cargo", "rustc"]).unwrap() {
            ProjectRuntimeImagePlan::Existing(image) => {
                assert_eq!(image.profile, RuntimeProfile::Rust);
                assert_eq!(image.platform, "linux/amd64");
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }

    #[test]
    fn polyglot_recipe_proposes_only_required_locked_sources() {
        let candidate = composite("amd64", &["cargo", "node", "rustc"]);
        assert_eq!(candidate.platform, "linux/amd64");
        assert_eq!(candidate.foundation.profile, RuntimeProfile::General);
        assert_eq!(
            candidate
                .components
                .iter()
                .map(|c| c.profile)
                .collect::<Vec<_>>(),
            vec![RuntimeProfile::Rust, RuntimeProfile::Node]
        );
        assert_eq!(candidate.required_tools, ["cargo", "node", "rustc"]);
        assert!(candidate.recipe_sha256.starts_with("sha256:"));
    }

    #[test]
    fn canonical_inputs_share_a_recipe_key_across_tasks() {
        let one = composite("amd64", &["node", "cargo", "node"]);
        let two = composite("amd64", &["cargo", "node"]);
        assert_eq!(one, two);
        assert_ne!(
            one.recipe_sha256,
            composite("arm64", &["cargo", "node"]).recipe_sha256
        );
        assert_ne!(
            one.recipe_sha256,
            composite("amd64", &["cargo", "python"]).recipe_sha256
        );
    }

    #[test]
    fn unapproved_tools_cannot_trigger_untrusted_downloads() {
        let error = plan("amd64", &["curl", "node"]).unwrap_err();
        assert_eq!(error.code, PrerequisiteCode::ProfileUnsupported);
        assert!(error.message.contains("curl"));
        assert!(!error.retryable);
    }

    #[test]
    fn unsupported_architecture_and_invalid_tool_id_fail_closed() {
        assert!(plan("mips64", &["node", "cargo"]).is_err());
        assert!(plan("amd64", &["", "cargo"]).is_err());
    }

    #[test]
    fn a_tool_free_recipe_uses_the_general_image() {
        match plan("amd64", &[]).unwrap() {
            ProjectRuntimeImagePlan::Existing(image) => {
                assert_eq!(image.profile, RuntimeProfile::General)
            }
            other => panic!("unexpected plan: {other:?}"),
        }
    }
}
