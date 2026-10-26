use crate::app::bootstrap_repair_policy::Operation;
use serde::{Deserialize, Serialize};

const OPERATION_CODE_VERSION: u16 = 2;

mod build;
mod descriptions;

pub(crate) use descriptions::RollbackGuidance;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PrivilegeLevel {
    CurrentUser,
    Administrator,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum ArtifactTrust {
    DigestPinned {
        sha256: String,
    },
    PublisherSigned {
        sha256: String,
        publisher: ApprovedPublisher,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ApprovedPublisher {
    Docker,
    Anthropic,
    Apple,
    Microsoft,
    Debian,
    Fedora,
    OpenSuse,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum ArtifactSource {
    BundledContainerImage {
        reference: String,
    },
    VendorInstaller {
        publisher: ApprovedPublisher,
        product: TrustedProduct,
    },
    ClaudeCodeRelease {
        publisher: ApprovedPublisher,
        product: TrustedProduct,
        version: String,
        manifest_url: String,
        signature_url: String,
        key_url: String,
        key_fingerprint: String,
        binary_url: String,
        target_path: String,
    },
    SignedPackageRepository {
        repository: TrustedRepository,
        product: TrustedProduct,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrustedProduct {
    DockerEngine,
    DockerDesktop,
    ClaudeCode,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TrustedRepository {
    Debian,
    Fedora,
    OpenSuse,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanEffect {
    RecheckPrerequisites,
    OpenCodingToolsSettings,
    DownloadLockedImage,
    RunIsolatedImageSmokeTest,
    DownloadSignedProviderManifest,
    VerifyProviderManifestSignature,
    DownloadManifestSelectedProviderBinary,
    InstallUserLocalProviderBinary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ManagedFileChange {
    DockerUserConfiguration,
    ClaudeCodeUserLocalBinary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ManagedServiceChange {
    DockerUserService,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct RemediationPlan {
    pub(crate) schema_version: u8,
    pub(crate) operation: Operation,
    pub(crate) operation_code_version: u16,
    pub(crate) host: crate::app::bootstrap_repair_policy::Host,
    pub(crate) os_version: String,
    pub(crate) prerequisite_id: String,
    pub(crate) parameter_digest: String,
    pub(crate) install_target: Option<String>,
    pub(crate) effects: Vec<PlanEffect>,
    pub(crate) file_changes: Vec<ManagedFileChange>,
    pub(crate) service_changes: Vec<ManagedServiceChange>,
    pub(crate) artifact_source: Option<ArtifactSource>,
    pub(crate) artifact_trust: Option<ArtifactTrust>,
    pub(crate) privilege: PrivilegeLevel,
    pub(crate) rollback: RollbackGuidance,
    pub(crate) verification_probe_ids: Vec<String>,
    pub(crate) requires_consent: bool,
    #[cfg(test)]
    test_override: bool,
}
