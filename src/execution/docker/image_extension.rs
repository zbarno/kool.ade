//! Validated requests and generated contexts for system image extensions.

use crate::{
    execution::docker::images::{ImageLock, RuntimeProfile},
    harness::DependencyDecision,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) mod builder;

const MAX_PACKAGES: usize = 64;
const MAX_PURPOSE_BYTES: usize = 2_048;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LinuxDistribution {
    DebianBookworm,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PackageTrust {
    ApprovedDistributionRepository,
    ThirdPartyRepository,
}

/// Untrusted proposal fields. Task/project identity must be attached by the
/// controller rather than resolved from worker-supplied paths.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SystemImageExtensionRequest {
    pub task_id: String,
    pub project_id: String,
    pub distribution: LinuxDistribution,
    pub base_profile: RuntimeProfile,
    pub base_image_digest: String,
    pub target_arch: String,
    pub packages: Vec<String>,
    pub trust: PackageTrust,
    pub purpose: String,
}

/// Validated proposal identity, not a cache key or executable build plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AuthorizedImageExtensionRequest {
    pub task_id: String,
    pub project_id: String,
    pub base_reference: String,
    pub base_digest: String,
    pub base_profile: RuntimeProfile,
    pub target_platform: String,
    pub requested_packages: Vec<String>,
    pub package_manifest: Vec<builder::PackageArtifactIdentity>,
    pub purpose: String,
    pub request_identity: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BaseImageSelection {
    pub reference: String,
    pub digest: String,
    pub profile: RuntimeProfile,
    pub platform: String,
}

/// Control-plane result after Docker completed the network-disabled build and
/// the installed package manifest was read back successfully.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DerivedImage {
    pub image_id: String,
    pub content_identity: String,
    pub base_digest: String,
    pub target_platform: String,
    pub package_manifest: Vec<builder::PackageArtifactIdentity>,
}

/// Mint a session-authenticated, one-time capability after controller-side
/// user authorization. It binds the full proposal, immutable package closure,
/// staged task, and transfer nonce.
pub(crate) fn sign_system_image_capability(
    key: &[u8; 32],
    transfer_id: &str,
    proposal: &SystemImageExtensionRequest,
    decision: DependencyDecision,
    workspace: &crate::execution::contract::WorkspaceRef,
    manifest: &[builder::PackageArtifactIdentity],
) -> anyhow::Result<String> {
    let canonical = serde_json::to_vec(&(transfer_id, proposal, decision, workspace, manifest))?;
    let mut inner_pad = [0x36_u8; 64];
    let mut outer_pad = [0x5c_u8; 64];
    for (index, byte) in key.iter().enumerate() {
        inner_pad[index] ^= byte;
        outer_pad[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(canonical);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);
    Ok(format!("hmac-sha256:{:x}", outer.finalize()))
}

pub(crate) fn verify_system_image_capability(
    key: &[u8; 32],
    token: &str,
    transfer_id: &str,
    proposal: &SystemImageExtensionRequest,
    decision: DependencyDecision,
    workspace: &crate::execution::contract::WorkspaceRef,
    manifest: &[builder::PackageArtifactIdentity],
) -> bool {
    let Ok(expected) =
        sign_system_image_capability(key, transfer_id, proposal, decision, workspace, manifest)
    else {
        return false;
    };
    expected.len() == token.len()
        && expected
            .bytes()
            .zip(token.bytes())
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
}

impl SystemImageExtensionRequest {
    /// Validate proposal scope and the locked base without granting any
    /// package or build capability. This supports displaying the exact
    /// repository closure before the user decides.
    pub(crate) fn resolve_base_selection(
        &self,
        expected_task_id: &str,
        expected_project_id: &str,
    ) -> anyhow::Result<BaseImageSelection> {
        validate_identifier(&self.task_id)?;
        validate_identifier(&self.project_id)?;
        anyhow::ensure!(
            self.task_id == expected_task_id && self.project_id == expected_project_id,
            "system image proposal is outside the selected task/project"
        );
        anyhow::ensure!(
            self.distribution == LinuxDistribution::DebianBookworm
                && self.trust == PackageTrust::ApprovedDistributionRepository,
            "only the approved Debian Bookworm archive is supported"
        );
        anyhow::ensure!(
            self.purpose.trim().len() >= 12
                && self.purpose.len() <= MAX_PURPOSE_BYTES
                && !self.purpose.chars().any(char::is_control),
            "system image extension purpose is invalid"
        );
        anyhow::ensure!(
            !self.packages.is_empty() && self.packages.len() <= MAX_PACKAGES,
            "system image extension package count is outside supported limits"
        );
        anyhow::ensure!(
            self.packages.windows(2).all(|pair| pair[0] < pair[1])
                && self
                    .packages
                    .iter()
                    .all(|package| valid_debian_package(package)),
            "system image extension package names are invalid or not canonical"
        );
        let selected = ImageLock::bundled()?
            .select(self.base_profile, &self.target_arch)
            .map_err(|error| {
                anyhow::anyhow!("approved base image is unavailable: {}", error.message)
            })?;
        anyhow::ensure!(
            selected.digest == self.base_image_digest,
            "system image extension base digest is not the approved locked image"
        );
        Ok(BaseImageSelection {
            reference: selected.reference,
            digest: selected.digest,
            profile: selected.profile,
            platform: selected.platform,
        })
    }

    /// Validate a proposal after an explicit task-scoped user grant.
    pub fn authorize(
        self,
        decision: DependencyDecision,
        expected_task_id: &str,
        package_manifest: Vec<builder::PackageArtifactIdentity>,
    ) -> anyhow::Result<AuthorizedImageExtensionRequest> {
        anyhow::ensure!(
            decision == DependencyDecision::UserAuthorizeForTask,
            "system image extension requires explicit task-scoped user authorization"
        );
        let selected = self.resolve_base_selection(expected_task_id, &self.project_id)?;
        anyhow::ensure!(
            !package_manifest.is_empty() && package_manifest.len() <= 256,
            "resolved system package closure is empty or exceeds its limit"
        );
        anyhow::ensure!(
            package_manifest
                .windows(2)
                .all(|pair| pair[0].name < pair[1].name)
                && self.packages.iter().all(|requested| package_manifest
                    .iter()
                    .any(|package| package.name == *requested)),
            "resolved system package closure is not canonical or omits a requested package"
        );
        for package in &package_manifest {
            builder::validate_identity(package, &selected.platform)?;
        }

        let identity_material = serde_json::to_vec(&(
            self.distribution,
            &selected.digest,
            &selected.platform,
            &package_manifest,
        ))?;
        let request_identity = format!("sha256:{:x}", Sha256::digest(identity_material));
        Ok(AuthorizedImageExtensionRequest {
            task_id: self.task_id,
            project_id: self.project_id,
            base_reference: selected.reference,
            base_digest: selected.digest,
            base_profile: self.base_profile,
            target_platform: selected.platform,
            requested_packages: self.packages,
            package_manifest,
            purpose: self.purpose,
            request_identity,
        })
    }
}

fn valid_debian_package(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"+.-".contains(&byte)
        })
        && value.as_bytes()[0].is_ascii_lowercase()
}

fn validate_identifier(value: &str) -> anyhow::Result<()> {
    crate::execution::contract::validate_identifier(value)
}

#[cfg(test)]
#[path = "image_extension/tests.rs"]
mod tests;
