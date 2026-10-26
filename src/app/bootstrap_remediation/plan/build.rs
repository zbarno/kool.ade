use super::*;
#[cfg(test)]
use crate::app::bootstrap_remediation::identity::HostIdentity;
use crate::app::bootstrap_remediation::request::{OperationParameters, RemediationRequest};
use crate::app::bootstrap_repair_policy::parse_locked_image_task_id;
use crate::execution::docker::images::{ImageLock, RuntimeProfile};
use sha2::{Digest, Sha256};

impl RemediationPlan {
    pub(crate) fn build(request: &RemediationRequest) -> anyhow::Result<Self> {
        request.validate()?;
        let plan = Self::from_request(request)?;
        plan.validate()?;
        Ok(plan)
    }

    #[cfg(test)]
    pub(crate) fn build_for_test(
        request: &RemediationRequest,
        identity: &HostIdentity,
    ) -> anyhow::Result<Self> {
        request.validate_for_identity(identity)?;
        let plan = Self::from_request(request)?;
        plan.validate()?;
        Ok(plan)
    }

    fn from_request(request: &RemediationRequest) -> anyhow::Result<Self> {
        let mut plan = Self {
            schema_version: 1,
            operation: request.operation,
            operation_code_version: OPERATION_CODE_VERSION,
            host: request.host,
            os_version: request.os_version.clone(),
            prerequisite_id: request.prerequisite_id.clone(),
            parameter_digest: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&request.parameters)?)
            ),
            effects: Vec::new(),
            file_changes: Vec::new(),
            service_changes: Vec::new(),
            artifact_source: None,
            artifact_trust: None,
            privilege: PrivilegeLevel::CurrentUser,
            rollback: RollbackGuidance::NoMutation,
            verification_probe_ids: vec![probe_for(&request.prerequisite_id)],
            requires_consent: false,
            #[cfg(test)]
            test_override: false,
        };
        match (&request.operation, &request.parameters) {
            (Operation::RecheckPrerequisites, OperationParameters::None) => {
                plan.effects.push(PlanEffect::RecheckPrerequisites);
            }
            (Operation::OpenCodingTools, OperationParameters::None) => {
                plan.effects.push(PlanEffect::OpenCodingToolsSettings);
            }
            (
                Operation::PrepareLockedImage,
                OperationParameters::LockedImage {
                    profile,
                    architecture,
                },
            ) => {
                let runtime_profile = match profile.as_str() {
                    "general" => RuntimeProfile::General,
                    "rust" => RuntimeProfile::Rust,
                    "node" => RuntimeProfile::Node,
                    "python" => RuntimeProfile::Python,
                    "go" => RuntimeProfile::Go,
                    _ => anyhow::bail!("unsupported locked image profile"),
                };
                let image = ImageLock::bundled()?.select(runtime_profile, architecture)?;
                plan.effects.extend([
                    PlanEffect::DownloadLockedImage,
                    PlanEffect::RunIsolatedImageSmokeTest,
                ]);
                plan.artifact_source = Some(ArtifactSource::BundledContainerImage {
                    reference: image.reference,
                });
                plan.artifact_trust = Some(ArtifactTrust::DigestPinned {
                    sha256: image.digest.trim_start_matches("sha256:").to_owned(),
                });
                plan.rollback = RollbackGuidance::KeepVerifiedArtifactForRetry;
                plan.verification_probe_ids.push("docker:rootless".into());
                plan.requires_consent = true;
            }
            _ => anyhow::bail!("remediation plan does not match its typed request"),
        }
        Ok(plan)
    }

    pub(crate) fn digest(&self) -> anyhow::Result<String> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)?;
        Ok(format!("{:x}", Sha256::digest(encoded)))
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        let parameters = parameters_from_plan(self)?;
        let request = RemediationRequest {
            schema_version: self.schema_version,
            operation: self.operation,
            host: self.host,
            os_version: self.os_version.clone(),
            prerequisite_id: self.prerequisite_id.clone(),
            parameters,
        };
        request.validate_shape()?;
        anyhow::ensure!(
            self.operation_code_version == OPERATION_CODE_VERSION,
            "unsupported remediation operation code version"
        );
        anyhow::ensure!(
            self.parameter_digest.len() == 64
                && self
                    .parameter_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit()),
            "invalid remediation parameter digest"
        );
        validate_artifact_policy(self)?;
        #[cfg(test)]
        if self.test_override {
            anyhow::ensure!(
                !self.effects.is_empty(),
                "test remediation plan has no effect"
            );
            anyhow::ensure!(
                !self.verification_probe_ids.is_empty(),
                "test remediation plan lacks a probe"
            );
            return Ok(());
        }
        let expected = Self::from_request(&request)?;
        anyhow::ensure!(
            *self == expected,
            "remediation plan differs from the app-owned operation catalog"
        );
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn set_privilege_for_test(&mut self, privilege: PrivilegeLevel) {
        self.privilege = privilege;
        self.test_override = true;
    }

    #[cfg(test)]
    pub(crate) fn set_artifact_for_test(&mut self, source: ArtifactSource, trust: ArtifactTrust) {
        self.artifact_source = Some(source);
        self.artifact_trust = Some(trust);
        self.test_override = true;
    }
}

fn validate_artifact_policy(plan: &RemediationPlan) -> anyhow::Result<()> {
    anyhow::ensure!(
        plan.artifact_source.is_some() == plan.artifact_trust.is_some(),
        "artifact source and trust policy must be paired"
    );
    let Some((source, trust)) = plan
        .artifact_source
        .as_ref()
        .zip(plan.artifact_trust.as_ref())
    else {
        return Ok(());
    };
    let (digest, publisher) = match trust {
        ArtifactTrust::DigestPinned { sha256 } => (sha256, None),
        ArtifactTrust::PublisherSigned { sha256, publisher } => (sha256, Some(*publisher)),
    };
    anyhow::ensure!(
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "invalid artifact SHA-256 digest"
    );
    match source {
        ArtifactSource::BundledContainerImage { reference } => {
            anyhow::ensure!(
                !reference.is_empty() && matches!(trust, ArtifactTrust::DigestPinned { .. }),
                "container images require the approved bundled registry and digest lock"
            );
        }
        ArtifactSource::VendorInstaller {
            publisher: source_publisher,
            ..
        } => {
            anyhow::ensure!(
                publisher == Some(*source_publisher),
                "vendor installers require that vendor's verified publisher signature"
            );
        }
        ArtifactSource::SignedPackageRepository { repository, .. } => {
            let required_publisher = match repository {
                TrustedRepository::Debian => ApprovedPublisher::Debian,
                TrustedRepository::Fedora => ApprovedPublisher::Fedora,
                TrustedRepository::OpenSuse => ApprovedPublisher::OpenSuse,
            };
            anyhow::ensure!(
                publisher == Some(required_publisher),
                "OS packages require the configured repository's verified publisher"
            );
        }
    }
    Ok(())
}

fn probe_for(prerequisite_id: &str) -> String {
    match prerequisite_id.strip_prefix("setup:") {
        Some(id) => id.to_owned(),
        None => "invalid".into(),
    }
}

fn parameters_from_plan(plan: &RemediationPlan) -> anyhow::Result<OperationParameters> {
    match plan.operation {
        Operation::RecheckPrerequisites | Operation::OpenCodingTools => {
            Ok(OperationParameters::None)
        }
        Operation::PrepareLockedImage => {
            let (profile, architecture, _) = parse_locked_image_task_id(&plan.prerequisite_id)
                .ok_or_else(|| anyhow::anyhow!("locked image task identity is not canonical"))?;
            Ok(OperationParameters::LockedImage {
                profile: profile.to_owned(),
                architecture: architecture.to_owned(),
            })
        }
    }
}
