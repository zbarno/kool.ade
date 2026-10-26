use super::*;
use serde::{Deserialize, Serialize};

impl PlanEffect {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::RecheckPrerequisites => "Run the typed prerequisite check again",
            Self::OpenCodingToolsSettings => "Open the existing coding tool settings",
            Self::DownloadLockedImage => "Download the exact image named by the bundled lock",
            Self::RunIsolatedImageSmokeTest => "Run a bounded, isolated image smoke test",
            Self::DownloadSignedProviderManifest => "Download the pinned provider release manifest",
            Self::VerifyProviderManifestSignature => {
                "Verify the manifest with the pinned Anthropic signing key"
            }
            Self::DownloadManifestSelectedProviderBinary => {
                "Download the exact binary selected by the verified manifest"
            }
            Self::InstallUserLocalProviderBinary => {
                "Install the verified Claude Code binary for this user"
            }
        }
    }
}

impl ManagedFileChange {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::DockerUserConfiguration => "Docker user configuration",
            Self::ClaudeCodeUserLocalBinary => "Claude Code user-local binary",
        }
    }
}

impl ManagedServiceChange {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::DockerUserService => "Docker user service",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RollbackGuidance {
    NoMutation,
    KeepVerifiedArtifactForRetry,
    KeepVerifiedProviderBinaryForRetry,
    ManualRecoveryRequired,
}

impl PrivilegeLevel {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::CurrentUser => "Runs as the current user; no elevation prompt",
            Self::Administrator => "Requires a separate OS administrator prompt",
        }
    }
}

impl ArtifactSource {
    pub(crate) fn description(&self) -> String {
        match self {
            Self::BundledContainerImage { reference } => {
                format!("bundled container image {reference}")
            }
            Self::VendorInstaller { publisher, product } => {
                format!("{publisher:?} installer for {product:?}")
            }
            Self::ClaudeCodeRelease {
                publisher,
                product,
                version,
                manifest_url,
                signature_url,
                key_url,
                key_fingerprint,
                binary_url,
                target_path,
            } => format!(
                "{publisher:?} {product:?} {version}; manifest {manifest_url}; signature {signature_url}; key {key_url} (fingerprint {key_fingerprint}); binary {binary_url}; install target {target_path}"
            ),
            Self::SignedPackageRepository {
                repository,
                product,
            } => {
                format!("{repository:?} package repository for {product:?}")
            }
        }
    }
}

impl ArtifactTrust {
    pub(crate) fn description(&self) -> String {
        match self {
            Self::DigestPinned { sha256 } => format!("SHA-256 {sha256}"),
            Self::PublisherSigned { sha256, publisher } => {
                format!("SHA-256 {sha256}, signed by {publisher:?}")
            }
        }
    }
}

impl RollbackGuidance {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::NoMutation => "No host configuration change is planned.",
            Self::KeepVerifiedArtifactForRetry => {
                "Keep the verified image for a later retry; this operation changes no host configuration."
            }
            Self::KeepVerifiedProviderBinaryForRetry => {
                "Keep a complete verified binary for retry; the final user-local path is created atomically and is never overwritten."
            }
            Self::ManualRecoveryRequired => {
                "Stop automatic retries and follow the supported manual recovery steps before another repair."
            }
        }
    }
}
