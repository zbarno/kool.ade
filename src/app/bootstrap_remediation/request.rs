use serde::{Deserialize, Serialize};

use crate::app::{
    bootstrap_remediation::identity::HostIdentity,
    bootstrap_repair_policy::{self, Host, Operation, Proposal},
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
            parameters: parameters_for(proposal)?,
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
            parameters: parameters_for(proposal)?,
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
                anyhow::ensure!(
                    self.prerequisite_id == format!("setup:images:{profile}:{architecture}"),
                    "locked image parameters do not match the affected prerequisite"
                );
            }
            _ => anyhow::bail!("remediation parameters do not match the typed operation"),
        }
        Ok(())
    }

    pub(crate) fn proposal(&self) -> Proposal {
        let image_profile = match &self.parameters {
            OperationParameters::LockedImage { profile, .. } => Some(profile.clone()),
            OperationParameters::None => None,
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

fn parameters_for(proposal: &Proposal) -> anyhow::Result<OperationParameters> {
    match proposal.operation {
        Operation::RecheckPrerequisites | Operation::OpenCodingTools => {
            Ok(OperationParameters::None)
        }
        Operation::PrepareLockedImage => {
            let profile = proposal
                .image_profile
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("locked image profile is missing"))?;
            let architecture = proposal
                .prerequisite_id
                .strip_prefix(&format!("setup:images:{profile}:"))
                .ok_or_else(|| anyhow::anyhow!("locked image prerequisite is not canonical"))?;
            Ok(OperationParameters::LockedImage {
                profile: profile.to_owned(),
                architecture: architecture.to_owned(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_request() -> RemediationRequest {
        let identity =
            HostIdentity::from_facts(Host::Linux, "6.12.1", "test-machine", "test-user").unwrap();
        RemediationRequest::from_proposal_for_test(
            &Proposal {
                schema_version: 1,
                operation: Operation::PrepareLockedImage,
                host: Host::Linux,
                prerequisite_id: "setup:images:general:amd64".into(),
                image_profile: Some("general".into()),
            },
            &identity,
        )
        .unwrap()
    }

    #[test]
    fn strict_request_round_trip_rejects_commands_and_unknown_parameters() {
        let request = image_request();
        let encoded = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<RemediationRequest>(&encoded).unwrap(),
            request
        );
        let injected = encoded.replace(
            "\"kind\":\"locked_image\"",
            "\"kind\":\"locked_image\",\"script\":\"sudo anything\"",
        );
        assert!(serde_json::from_str::<RemediationRequest>(&injected).is_err());
        assert!(serde_json::from_str::<RemediationRequest>(
            r#"{"schemaVersion":1,"operation":"run_host_command","host":"linux","osVersion":"6.12","prerequisiteId":"setup:docker","parameters":{"kind":"none"}}"#
        ).is_err());
    }

    #[test]
    fn parameters_host_and_os_version_are_revalidated() {
        let mut request = image_request();
        request.parameters = OperationParameters::None;
        assert!(request.validate_shape().is_err());
        let mut request = image_request();
        request.os_version = "6.12\nmalicious".into();
        assert!(request.validate_shape().is_err());
        let mut request = image_request();
        request.prerequisite_id = "setup:../../escape".into();
        assert!(request.validate_shape().is_err());
        let mut request = image_request();
        if let OperationParameters::LockedImage { architecture, .. } = &mut request.parameters {
            *architecture = "arm64".into();
        }
        assert!(request.validate_shape().is_err());
    }

    #[test]
    fn request_requires_verified_current_host_and_os_version() {
        let request = image_request();
        assert!(
            request
                .validate_for_identity(&HostIdentity::current().unwrap())
                .is_err()
        );
    }
}
