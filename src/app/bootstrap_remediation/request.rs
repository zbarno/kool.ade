use serde::{Deserialize, Serialize};

use crate::app::{
    bootstrap_remediation::identity::HostIdentity,
    bootstrap_repair_policy::{self, Host, Operation, Proposal, parse_locked_image_task_id},
    setup_attention::catalog::LinuxRootlessRecipeV1,
};

/// A strict request shape. All executable details are absent by construction.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct RemediationRequest {
    pub(crate) schema_version: u8,
    pub(crate) operation: Operation,
    pub(crate) host: Host,
    pub(crate) os_version: String,
    pub(crate) prerequisite_id: String,
    pub(crate) parameters: OperationParameters,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields, rename_all = "snake_case")]
pub(crate) enum OperationParameters {
    None,
    LockedImage {
        profile: String,
        architecture: String,
    },
    InstallLinuxRootlessPackages {
        recipe: LinuxRootlessRecipeV1,
    },
    ConfigureLinuxSubordinateIds {
        recipe: LinuxRootlessRecipeV1,
    },
    InstallLinuxRootlessService {
        recipe: LinuxRootlessRecipeV1,
    },
}

impl RemediationRequest {
    pub(crate) fn from_proposal(proposal: &Proposal) -> anyhow::Result<Self> {
        let identity = HostIdentity::current()?;
        anyhow::ensure!(
            proposal.host == identity.host(),
            "proposal targets another host"
        );
        let request = Self {
            schema_version: 1,
            operation: proposal.operation,
            host: proposal.host,
            os_version: identity.os_version().to_owned(),
            prerequisite_id: proposal.prerequisite_id.clone(),
            parameters: parameters_for(proposal, identity.os_version())?,
        };
        request.validate_for_identity(&identity)?;
        Ok(request)
    }

    #[cfg(test)]
    pub(crate) fn from_proposal_for_test(
        proposal: &Proposal,
        identity: &HostIdentity,
    ) -> anyhow::Result<Self> {
        let request = Self {
            schema_version: 1,
            operation: proposal.operation,
            host: proposal.host,
            os_version: identity.os_version().to_owned(),
            prerequisite_id: proposal.prerequisite_id.clone(),
            parameters: parameters_for(proposal, identity.os_version())?,
        };
        request.validate_for_identity(&identity.clone())?;
        Ok(request)
    }

    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        self.validate_for_identity(&HostIdentity::current()?)
    }

    pub(crate) fn validate_for_identity(&self, identity: &HostIdentity) -> anyhow::Result<()> {
        self.validate_shape()?;
        anyhow::ensure!(
            self.host == identity.host() && self.os_version == identity.os_version(),
            "remediation request does not match the current host OS"
        );
        Ok(())
    }

    pub(crate) fn validate_shape(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported remediation request version"
        );
        anyhow::ensure!(
            !self.os_version.is_empty()
                && self.os_version.len() <= 64
                && self.os_version.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+')
                }),
            "invalid host OS version"
        );
        bootstrap_repair_policy::validate(&self.proposal(), self.host)?;
        match (&self.operation, &self.parameters) {
            (
                Operation::RecheckPrerequisites | Operation::OpenCodingTools,
                OperationParameters::None,
            ) => {}
            (
                Operation::PrepareLockedImage,
                OperationParameters::LockedImage {
                    profile,
                    architecture,
                },
            ) => {
                anyhow::ensure!(
                    matches!(
                        profile.as_str(),
                        "general" | "rust" | "node" | "python" | "go"
                    ),
                    "unsupported locked image profile"
                );
                anyhow::ensure!(
                    matches!(architecture.as_str(), "amd64" | "arm64"),
                    "unsupported locked image architecture"
                );
                let (task_profile, task_architecture, _) =
                    parse_locked_image_task_id(&self.prerequisite_id).ok_or_else(|| {
                        anyhow::anyhow!("locked image prerequisite is not canonical")
                    })?;
                anyhow::ensure!(
                    task_profile == profile.as_str() && task_architecture == architecture.as_str(),
                    "locked image parameters do not match the affected prerequisite"
                );
            }
            (
                Operation::InstallLinuxRootlessPackages,
                OperationParameters::InstallLinuxRootlessPackages { recipe },
            )
            | (
                Operation::ConfigureLinuxSubordinateIds,
                OperationParameters::ConfigureLinuxSubordinateIds { recipe },
            )
            | (
                Operation::InstallLinuxRootlessService,
                OperationParameters::InstallLinuxRootlessService { recipe },
            ) => {
                let prefix = recipe.os_version_prefix();
                anyhow::ensure!(
                    LinuxRootlessRecipeV1::from_os_version(&self.os_version) == Some(*recipe),
                    "rootless Docker recipe does not match the exact host identity"
                );
                anyhow::ensure!(
                    self.os_version
                        .strip_prefix(prefix)
                        .is_some_and(|kernel| !kernel.is_empty()),
                    "rootless Docker operation does not match its exact distro recipe"
                );
                let kernel = self
                    .os_version
                    .strip_prefix(prefix)
                    .expect("rootless recipe identity was checked above");
                anyhow::ensure!(
                    kernel_meets_rootless_floor(kernel),
                    "rootless Docker operation requires Linux kernel 4.15 or newer"
                );
            }
            _ => anyhow::bail!("remediation parameters do not match the typed operation"),
        }
        Ok(())
    }

    pub(crate) fn proposal(&self) -> Proposal {
        let image_profile = match &self.parameters {
            OperationParameters::LockedImage { profile, .. } => Some(profile.clone()),
            OperationParameters::None
            | OperationParameters::InstallLinuxRootlessPackages { .. }
            | OperationParameters::ConfigureLinuxSubordinateIds { .. }
            | OperationParameters::InstallLinuxRootlessService { .. } => None,
        };
        Proposal {
            schema_version: self.schema_version,
            operation: self.operation,
            host: self.host,
            prerequisite_id: self.prerequisite_id.clone(),
            image_profile,
        }
    }
}

fn kernel_meets_rootless_floor(release: &str) -> bool {
    let mut components = release.split(['.', '-']);
    let Some(major) = components
        .next()
        .and_then(|value| value.parse::<u16>().ok())
    else {
        return false;
    };
    let Some(minor) = components
        .next()
        .and_then(|value| value.parse::<u16>().ok())
    else {
        return false;
    };
    major > 4 || (major == 4 && minor >= 15)
}

fn parameters_for(proposal: &Proposal, os_version: &str) -> anyhow::Result<OperationParameters> {
    let recipe = LinuxRootlessRecipeV1::from_os_version(os_version);
    match proposal.operation {
        Operation::RecheckPrerequisites | Operation::OpenCodingTools => {
            Ok(OperationParameters::None)
        }
        Operation::PrepareLockedImage => {
            let profile = proposal
                .image_profile
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("locked image profile is missing"))?;
            let (task_profile, architecture, _) =
                parse_locked_image_task_id(&proposal.prerequisite_id)
                    .ok_or_else(|| anyhow::anyhow!("locked image prerequisite is not canonical"))?;
            anyhow::ensure!(
                task_profile == profile,
                "locked image profile does not match the affected prerequisite"
            );
            Ok(OperationParameters::LockedImage {
                profile: task_profile.to_owned(),
                architecture: architecture.to_owned(),
            })
        }
        Operation::InstallLinuxRootlessPackages => {
            let recipe = recipe.ok_or_else(|| {
                anyhow::anyhow!("Linux host identity has no supported rootless recipe")
            })?;
            Ok(OperationParameters::InstallLinuxRootlessPackages { recipe })
        }
        Operation::ConfigureLinuxSubordinateIds => {
            let recipe = recipe.ok_or_else(|| {
                anyhow::anyhow!("Linux host identity has no supported rootless recipe")
            })?;
            Ok(OperationParameters::ConfigureLinuxSubordinateIds { recipe })
        }
        Operation::InstallLinuxRootlessService => {
            let recipe = recipe.ok_or_else(|| {
                anyhow::anyhow!("Linux host identity has no supported rootless recipe")
            })?;
            Ok(OperationParameters::InstallLinuxRootlessService { recipe })
        }
    }
}

#[cfg(test)]
#[path = "request/tests.rs"]
mod tests;
