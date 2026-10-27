use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::discovery::{PrerequisiteCode, PrerequisiteFailure, parse_architecture};

mod observation;
mod validation;
pub use observation::{
    approved_available_images, verify_derived_image_observation, verify_image_observation,
};

#[cfg(not(feature = "qa-desktop-driver-image"))]
pub const IMAGE_LOCK: &str = include_str!("../../../containers/images.lock.json");
pub const EXECUTOR_UID: u32 = 10_001;
pub const EXECUTOR_GID: u32 = 10_001;
const PINNED_RUST_TOOLCHAIN: &str = "1.98.1";
const PINNED_PYTHON_RUNTIME: &str = "3.14.7";
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
        {
            return Err(failure(
                PrerequisiteCode::ProfileUnsupported,
                "QA runtime remains disabled until its derived image digest and isolated Docker lifecycle are approved",
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

    fn variant(
        &self,
        profile: RuntimeProfile,
        architecture: &str,
    ) -> Result<&ImageVariant, PrerequisiteFailure> {
        let profile_name = match profile {
            RuntimeProfile::General => "general",
            RuntimeProfile::Rust => "rust",
            RuntimeProfile::Node => "node",
            RuntimeProfile::Python => "python",
            RuntimeProfile::Go => "go",
            RuntimeProfile::BrowserQa => "browser_qa",
            RuntimeProfile::DesktopQa => "desktop_qa",
        };
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
            if required_tools
                .iter()
                .all(|tool| image.tool_versions.contains_key(tool))
            {
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
