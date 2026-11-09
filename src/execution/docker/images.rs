use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::discovery::{PrerequisiteCode, PrerequisiteFailure, parse_architecture};

mod composition;
mod composition_cache;
mod observation;
mod project_plan;
mod project_tools;
mod runtime_binding;
mod validation;
pub(crate) use composition::verified_context as verified_composition_context;
pub(crate) use composition_cache::{
    CompositionReceipt, try_acquire_process_lock as try_acquire_composition_lock,
};
pub use observation::{
    approved_available_images, verify_derived_image_observation, verify_image_observation,
    verify_qa_image_observation,
};
pub(crate) use project_plan::{CompositionCandidate, ProjectRuntimeImagePlan};
pub use project_tools::ProjectRuntimeTools;
pub(crate) use project_tools::active_target_recipe_tools;
pub use runtime_binding::RuntimePlanBinding;

#[cfg(not(feature = "qa-desktop-driver-image"))]
pub const IMAGE_LOCK: &str = include_str!("../../../containers/images.lock.json");
pub const EXECUTOR_UID: u32 = 10_001;
pub const EXECUTOR_GID: u32 = 10_001;
const PINNED_RUST_TOOLCHAIN: &str = "1.98.1";
pub(crate) const PINNED_PYTHON_RUNTIME: &str = "3.14.7";
const PINNED_GO_RUNTIME: &str = "1.26.8";
const TRUSTED_BROWSER_QA_REPOSITORY: &str = "ghcr.io/zbarno/koolade-runtime-qa-browser";
const TRUSTED_DESKTOP_QA_REPOSITORY: &str = "ghcr.io/zbarno/koolade-runtime-qa-desktop";

pub(crate) const DESKTOP_CAPACITY_PROBE_MAX_PULL_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const DESKTOP_CAPACITY_PROBE_MAX_STORAGE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeProfile {
    General,
    Rust,
    Node,
    Python,
    Go,
    BrowserQa,
    DesktopQa,
}

pub(crate) fn task_runtime_profile_order(required_tools: &[String]) -> &'static [RuntimeProfile] {
    const GENERIC_FIRST: &[RuntimeProfile] = &[
        RuntimeProfile::General,
        RuntimeProfile::Node,
        RuntimeProfile::Rust,
        RuntimeProfile::Python,
        RuntimeProfile::Go,
        RuntimeProfile::BrowserQa,
    ];
    const TOOL_FIRST: &[RuntimeProfile] = &[
        RuntimeProfile::Node,
        RuntimeProfile::Rust,
        RuntimeProfile::Python,
        RuntimeProfile::Go,
        RuntimeProfile::General,
        RuntimeProfile::BrowserQa,
    ];

    if required_tools.is_empty() {
        GENERIC_FIRST
    } else {
        TOOL_FIRST
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImageVariant {
    pub platform: String,
    pub reference: String,
    pub digest: String,
    pub tool_versions: BTreeMap<String, String>,
    #[serde(default)]
    pub runtime_enabled: bool,
    #[serde(default)]
    pub build_recipe_sha256: Option<String>,
    /// Sum of the digest-bound config and compressed layer blobs. Actual
    /// transfer is usually lower when Docker already has shared layers.
    #[serde(default)]
    pub estimated_pull_bytes: Option<u64>,
    /// Approximate storage reserve using twice the compressed manifest size.
    #[serde(default)]
    pub estimated_storage_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageSizeEstimate {
    pub estimated_pull_bytes: u64,
    pub estimated_storage_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImageLock {
    pub schema_version: u16,
    pub image_set_version: String,
    pub profiles: BTreeMap<String, Vec<ImageVariant>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedImage {
    pub profile: RuntimeProfile,
    pub platform: String,
    pub reference: String,
    pub digest: String,
    pub runtime_user: String,
    pub tool_versions: BTreeMap<String, String>,
}

impl SelectedImage {
    /// Check a logical command against the image's canonical capability keys.
    /// Some images publish the common `python` capability while exposing both
    /// `python` and `python3` command names.
    pub fn provides_tool(&self, tool: &str) -> bool {
        self.tool_versions.contains_key(tool)
            || (tool == "python3" && self.tool_versions.contains_key("python"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageObservation {
    pub image_id: String,
    pub repo_digests: Vec<String>,
    pub os: String,
    pub architecture: String,
    pub configured_user: String,
    pub labels: BTreeMap<String, String>,
}

impl ImageLock {
    pub fn bundled() -> Result<Self, PrerequisiteFailure> {
        #[cfg(feature = "qa-desktop-driver-image")]
        return Err(failure(
            PrerequisiteCode::ProfileUnsupported,
            "Executor image selection is unavailable in the isolated desktop driver build",
            false,
        ));

        #[cfg(not(feature = "qa-desktop-driver-image"))]
        {
            let lock: Self = serde_json::from_str(IMAGE_LOCK).map_err(|_| {
                failure(
                    PrerequisiteCode::ImageUnavailable,
                    "Bundled executor image manifest is invalid",
                    false,
                )
            })?;
            lock.validate()?;
            Ok(lock)
        }
    }

    pub fn select(
        &self,
        profile: RuntimeProfile,
        daemon_arch: &str,
    ) -> Result<SelectedImage, PrerequisiteFailure> {
        self.select_inner(profile, daemon_arch, false)
    }

    pub(crate) fn select_experimental_qa(
        &self,
        profile: RuntimeProfile,
        daemon_arch: &str,
        settings: &crate::persistence::experimental_features::ExperimentalFeatureSettingsV1,
    ) -> Result<SelectedImage, PrerequisiteFailure> {
        let feature = match profile {
            RuntimeProfile::BrowserQa => {
                crate::persistence::experimental_features::ExperimentalFeature::BrowserQa
            }
            RuntimeProfile::DesktopQa => {
                crate::persistence::experimental_features::ExperimentalFeature::LinuxDesktopQa
            }
            _ => return self.select(profile, daemon_arch),
        };
        if !settings.feature_enabled(feature) {
            return Err(failure(
                PrerequisiteCode::ProfileUnsupported,
                "QA runtime remains disabled by release policy; enable local Experimental Mode and this feature to use its pinned digest for dogfooding",
                false,
            ));
        }
        self.select_inner(profile, daemon_arch, true)
    }

    fn select_inner(
        &self,
        profile: RuntimeProfile,
        daemon_arch: &str,
        experimental_authorized: bool,
    ) -> Result<SelectedImage, PrerequisiteFailure> {
        let architecture = parse_architecture(daemon_arch).ok_or_else(|| {
            failure(
                PrerequisiteCode::UnsupportedArchitecture,
                "No approved executor image exists for this Docker architecture",
                false,
            )
        })?;
        let variant = self.variant(profile, architecture)?;
        if matches!(
            profile,
            RuntimeProfile::BrowserQa | RuntimeProfile::DesktopQa
        ) && !variant.runtime_enabled
            && !experimental_authorized
        {
            return Err(failure(
                PrerequisiteCode::ProfileUnsupported,
                "QA runtime remains disabled until release approval or explicit local experimental authorization",
                false,
            ));
        }
        let platform = format!("linux/{architecture}");
        Ok(SelectedImage {
            profile,
            platform,
            reference: variant.reference.clone(),
            digest: variant.digest.clone(),
            runtime_user: format!("{EXECUTOR_UID}:{EXECUTOR_GID}"),
            tool_versions: variant.tool_versions.clone(),
        })
    }

    pub fn size_estimate(
        &self,
        profile: RuntimeProfile,
        daemon_arch: &str,
    ) -> Result<ImageSizeEstimate, PrerequisiteFailure> {
        let architecture = parse_architecture(daemon_arch).ok_or_else(|| {
            failure(
                PrerequisiteCode::UnsupportedArchitecture,
                "No approved executor image exists for this Docker architecture",
                false,
            )
        })?;
        let variant = self.variant(profile, architecture)?;
        let estimated_pull_bytes = variant.estimated_pull_bytes.ok_or_else(|| {
            failure(
                PrerequisiteCode::ImageUnavailable,
                "The approved image has no digest-bound transfer estimate",
                false,
            )
        })?;
        let estimated_storage_bytes = variant.estimated_storage_bytes.ok_or_else(|| {
            failure(
                PrerequisiteCode::ImageUnavailable,
                "The approved image has no digest-bound storage estimate",
                false,
            )
        })?;
        Ok(ImageSizeEstimate {
            estimated_pull_bytes,
            estimated_storage_bytes,
        })
    }

    /// Conservative worst-case totals for a proposed composition: sums the
    /// digest-bound estimates of the foundation and every component image.
    /// Shared layers make real growth smaller, never larger. Returns
    /// `None` when any part lacks an estimate; callers must treat that as
    /// "unknown" (never zero) and avoid publishing numeric progress.
    pub(crate) fn composition_size_estimate(
        &self,
        candidate: &project_plan::CompositionCandidate,
    ) -> Option<ImageSizeEstimate> {
        let mut estimated_pull_bytes = 0_u64;
        let mut estimated_storage_bytes = 0_u64;
        for selected in std::iter::once(&candidate.foundation).chain(candidate.components.iter()) {
            let variant = self
                .profiles
                .get(Self::profile_name(selected.profile))?
                .iter()
                .find(|variant| {
                    variant.platform == selected.platform && variant.digest == selected.digest
                })?;
            estimated_pull_bytes =
                estimated_pull_bytes.saturating_add(variant.estimated_pull_bytes?);
            estimated_storage_bytes =
                estimated_storage_bytes.saturating_add(variant.estimated_storage_bytes?);
        }
        Some(ImageSizeEstimate {
            estimated_pull_bytes,
            estimated_storage_bytes,
        })
    }

    fn profile_name(profile: RuntimeProfile) -> &'static str {
        match profile {
            RuntimeProfile::General => "general",
            RuntimeProfile::Rust => "rust",
            RuntimeProfile::Node => "node",
            RuntimeProfile::Python => "python",
            RuntimeProfile::Go => "go",
            RuntimeProfile::BrowserQa => "browser_qa",
            RuntimeProfile::DesktopQa => "desktop_qa",
        }
    }

    fn variant(
        &self,
        profile: RuntimeProfile,
        architecture: &str,
    ) -> Result<&ImageVariant, PrerequisiteFailure> {
        let profile_name = Self::profile_name(profile);
        let platform = format!("linux/{architecture}");
        self.profiles
            .get(profile_name)
            .and_then(|variants| variants.iter().find(|variant| variant.platform == platform))
            .ok_or_else(|| {
                failure(
                    PrerequisiteCode::ImageUnavailable,
                    "No approved executor image is locked for this profile and architecture",
                    false,
                )
            })
    }

    /// Select a locked runtime that advertises every logical tool required by
    /// an approved, stack-neutral recipe. A profile disabled by policy remains
    /// unavailable even when its tool metadata would otherwise match.
    pub fn select_with_tools(
        &self,
        profiles: &[RuntimeProfile],
        daemon_arch: &str,
        required_tools: &[String],
    ) -> Result<SelectedImage, PrerequisiteFailure> {
        for profile in profiles {
            let Ok(image) = self.select(*profile, daemon_arch) else {
                continue;
            };
            if required_tools.iter().all(|tool| image.provides_tool(tool)) {
                return Ok(image);
            }
        }
        Err(failure(
            PrerequisiteCode::ProfileUnsupported,
            "No enabled, pinned runtime image provides all tools required by this approved recipe",
            false,
        ))
    }
}

#[cfg(test)]
#[path = "images/tests.rs"]
mod tests;

pub fn is_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn failure(code: PrerequisiteCode, message: &str, retryable: bool) -> PrerequisiteFailure {
    PrerequisiteFailure {
        code,
        message: message.into(),
        retryable,
    }
}
