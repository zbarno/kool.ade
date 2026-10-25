use super::*;
use serde::{Deserialize, Serialize};

impl PlanEffect {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::RecheckPrerequisites => "Run the typed prerequisite check again",
            Self::OpenCodingToolsSettings => "Open the existing coding tool settings",
            Self::DownloadLockedImage => "Download the exact image named by the bundled lock",
            Self::RunIsolatedImageSmokeTest => "Run a bounded, isolated image smoke test",
        }
    }
}

impl ManagedFileChange {
    pub(crate) fn description(self) -> &'static str {
        match self {
            Self::DockerUserConfiguration => "Docker user configuration",
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
            Self::ManualRecoveryRequired => {
                "Stop automatic retries and follow the supported manual recovery steps before another repair."
            }
        }
    }
}
