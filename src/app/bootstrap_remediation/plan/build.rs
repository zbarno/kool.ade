use super::*;
#[cfg(test)]
use crate::app::bootstrap_remediation::identity::HostIdentity;
use crate::app::bootstrap_remediation::request::{OperationParameters, RemediationRequest};
use crate::execution::docker::images::{ImageLock, RuntimeProfile};
use sha2::{Digest, Sha256};

mod rootless;
mod validation;

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
            install_target: None,
            effects: Vec::new(),
            package_changes: Vec::new(),
            file_changes: Vec::new(),
            service_changes: Vec::new(),
            precondition_probe_ids: Vec::new(),
            artifact_source: None,
            artifact_trust: None,
            privilege: PrivilegeLevel::CurrentUser,
            rollback: RollbackGuidance::NoMutation,
            verification_probe_ids: vec![validation::probe_for(&request.prerequisite_id)],
            required_consents: Vec::new(),
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
            (
                Operation::InstallClaudeCode,
                OperationParameters::ClaudeCodeInstall { target_path },
            ) => {
                use super::super::claude_code_install::recipe;

                let target = recipe::validate_target(target_path)?;
                plan.install_target = Some(target_path.clone());
                plan.effects.extend([
                    PlanEffect::DownloadSignedProviderManifest,
                    PlanEffect::VerifyProviderManifestSignature,
                    PlanEffect::DownloadManifestSelectedProviderBinary,
                    PlanEffect::InstallUserLocalProviderBinary,
                ]);
                plan.file_changes
                    .push(ManagedFileChange::ClaudeCodeUserLocalBinary);
                plan.artifact_source = Some(ArtifactSource::ClaudeCodeRelease {
                    publisher: ApprovedPublisher::Anthropic,
                    product: TrustedProduct::ClaudeCode,
                    version: recipe::RELEASE.to_owned(),
                    manifest_url: recipe::MANIFEST_URL.to_owned(),
                    signature_url: recipe::SIGNATURE_URL.to_owned(),
                    key_url: recipe::KEY_URL.to_owned(),
                    key_fingerprint: recipe::KEY_FINGERPRINT.to_owned(),
                    binary_url: recipe::BINARY_URL.to_owned(),
                    target_path: target.display().to_string(),
                });
                plan.artifact_trust = Some(ArtifactTrust::PublisherSigned {
                    sha256: recipe::BINARY_SHA256.to_owned(),
                    publisher: ApprovedPublisher::Anthropic,
                });
                plan.rollback = RollbackGuidance::KeepVerifiedProviderBinaryForRetry;
                plan.verification_probe_ids = vec!["cli:claude:install".into()];
                plan.requires_consent = true;
            }
            (
                Operation::InstallLinuxRootlessPackages,
                OperationParameters::InstallLinuxRootlessPackages { recipe },
            ) => rootless::install_packages(&mut plan, *recipe)?,
            (
                Operation::ConfigureLinuxSubordinateIds,
                OperationParameters::ConfigureLinuxSubordinateIds { recipe },
            ) => rootless::configure_subordinate_ids(&mut plan, *recipe)?,
            (
                Operation::InstallLinuxRootlessService,
                OperationParameters::InstallLinuxRootlessService { recipe },
            ) => rootless::install_service(&mut plan, *recipe)?,
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
        let parameters = validation::parameters_from_plan(self)?;
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
        validation::validate_artifact_policy(self)?;
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

    pub(crate) fn validate_current_target(&self) -> anyhow::Result<()> {
        if self.operation == Operation::InstallClaudeCode {
            super::super::claude_code_install::recipe::require_supported_host()?;
            let expected = super::super::claude_code_install::recipe::current_target()?;
            anyhow::ensure!(
                self.install_target.as_deref() == Some(expected.as_str()),
                "Claude Code install target changed after review"
            );
        }
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
