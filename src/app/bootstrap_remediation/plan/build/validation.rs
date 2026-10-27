use super::super::*;
use crate::app::bootstrap_remediation::request::OperationParameters;
use crate::app::bootstrap_repair_policy::parse_locked_image_task_id;
use crate::app::setup_attention::catalog::LinuxRootlessRecipeV1;

pub(super) fn validate_artifact_policy(plan: &RemediationPlan) -> anyhow::Result<()> {
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
    let (digest, trusted_publisher) = match trust {
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
                trusted_publisher == Some(*source_publisher),
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
                trusted_publisher == Some(required_publisher),
                "OS packages require the configured repository's verified publisher"
            );
        }
        ArtifactSource::ClaudeCodeRelease {
            publisher,
            product,
            version,
            manifest_url,
            signature_url,
            key_url,
            key_fingerprint,
            binary_url,
            target_path,
        } => {
            use crate::app::bootstrap_remediation::claude_code_install::recipe;

            anyhow::ensure!(
                *publisher == ApprovedPublisher::Anthropic
                    && *product == TrustedProduct::ClaudeCode
                    && version == recipe::RELEASE
                    && manifest_url == recipe::MANIFEST_URL
                    && signature_url == recipe::SIGNATURE_URL
                    && key_url == recipe::KEY_URL
                    && key_fingerprint == recipe::KEY_FINGERPRINT
                    && binary_url == recipe::BINARY_URL
                    && target_path == plan.install_target.as_deref().unwrap_or_default()
                    && trusted_publisher == Some(ApprovedPublisher::Anthropic)
                    && digest == recipe::BINARY_SHA256,
                "Claude Code release source or publisher trust differs from its pinned app recipe"
            );
        }
    }
    Ok(())
}

pub(super) fn probe_for(prerequisite_id: &str) -> String {
    match prerequisite_id.strip_prefix("setup:") {
        Some(id) => id.to_owned(),
        None => "invalid".into(),
    }
}

pub(super) fn parameters_from_plan(plan: &RemediationPlan) -> anyhow::Result<OperationParameters> {
    let rootless_recipe = || {
        LinuxRootlessRecipeV1::from_os_version(&plan.os_version)
            .ok_or_else(|| anyhow::anyhow!("plan host has no exact rootless recipe"))
    };
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
        Operation::InstallClaudeCode => Ok(OperationParameters::ClaudeCodeInstall {
            target_path: plan
                .install_target
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Claude install target is missing"))?,
        }),
        Operation::InstallLinuxRootlessPackages => {
            Ok(OperationParameters::InstallLinuxRootlessPackages {
                recipe: rootless_recipe()?,
            })
        }
        Operation::ConfigureLinuxSubordinateIds => {
            Ok(OperationParameters::ConfigureLinuxSubordinateIds {
                recipe: rootless_recipe()?,
            })
        }
        Operation::InstallLinuxRootlessService => {
            Ok(OperationParameters::InstallLinuxRootlessService {
                recipe: rootless_recipe()?,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        bootstrap_remediation::{identity::HostIdentity, request::RemediationRequest},
        bootstrap_repair_policy::{Host, Operation, Proposal},
    };

    fn claude_plan() -> RemediationPlan {
        let identity =
            HostIdentity::from_facts(Host::Linux, "6.12", "synthetic-machine", "synthetic-user")
                .unwrap();
        let request = RemediationRequest::from_proposal_for_test(
            &Proposal {
                schema_version: 1,
                operation: Operation::InstallClaudeCode,
                host: Host::Linux,
                prerequisite_id: "setup:cli:claude:install".into(),
                image_profile: None,
            },
            &identity,
        )
        .unwrap();
        RemediationPlan::build_for_test(&request, &identity).unwrap()
    }

    #[test]
    fn claude_plan_binds_one_current_user_target_and_the_exact_release_source() {
        let plan = claude_plan();
        assert_eq!(plan.privilege, PrivilegeLevel::CurrentUser);
        assert!(plan.service_changes.is_empty());
        let source_target = match plan.artifact_source.as_ref().unwrap() {
            ArtifactSource::ClaudeCodeRelease { target_path, .. } => target_path.as_str(),
            _ => panic!("Claude plan lacks its pinned release source"),
        };
        assert_eq!(plan.install_target.as_deref(), Some(source_target));
        assert!(plan.validate().is_ok());

        let mut changed_source = plan.clone();
        if let Some(ArtifactSource::ClaudeCodeRelease { binary_url, .. }) =
            &mut changed_source.artifact_source
        {
            *binary_url = "https://example.invalid/claude".into();
        }
        assert!(changed_source.validate().is_err());

        let mut elevated = plan;
        elevated.privilege = PrivilegeLevel::Administrator;
        assert!(elevated.validate().is_err());
    }
}
