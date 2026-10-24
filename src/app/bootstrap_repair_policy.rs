//! Pure policy boundary for the future machine bootstrap repair broker.
//!
//! A coding agent may suggest an operation ID, but cannot supply a host command
//! or execute a mutation. The controller must still obtain a one-use approval,
//! resolve a vetted native implementation, and independently recheck readiness.
//! No operation executor is available in this module.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Host {
    Linux,
    MacOs,
    Windows,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    /// Verify only, no host mutation or external executable.
    RecheckPrerequisites,
    /// A reviewed digest from the bundled runtime image lock; no freeform image.
    PrepareLockedImage,
    /// Open existing application settings, never a process execution.
    OpenCodingTools,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Proposal {
    pub schema_version: u8,
    pub operation: Operation,
    pub host: Host,
    pub prerequisite_id: String,
    /// Optional image profile name, not a registry reference or a command.
    pub image_profile: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewedAction {
    pub operation: Operation,
    pub prerequisite_id: String,
    pub requires_consent: bool,
    pub requires_engine_verification: bool,
}

/// Validate intent only. No host process, Docker action, or permission grant
/// occurs here. An executable remediation engine must enforce this again
/// against fresh host evidence and a separate, real user approval.
pub fn validate(proposal: &Proposal, detected_host: Host) -> anyhow::Result<ReviewedAction> {
    anyhow::ensure!(proposal.schema_version == 1, "unsupported repair proposal version");
    anyhow::ensure!(proposal.host == detected_host, "repair proposal host changed");
    anyhow::ensure!(
        proposal.prerequisite_id.starts_with("setup:")
            && proposal.prerequisite_id.len() <= 180
            && proposal.prerequisite_id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_')
            }),
        "invalid repair prerequisite identity"
    );
    let (requires_consent, requires_engine_verification) = match proposal.operation {
        Operation::RecheckPrerequisites => {
            anyhow::ensure!(proposal.image_profile.is_none(), "unexpected image profile");
            (false, false)
        }
        Operation::OpenCodingTools => {
            anyhow::ensure!(proposal.image_profile.is_none(), "unexpected image profile");
            anyhow::ensure!(
                proposal.prerequisite_id.starts_with("setup:cli:"),
                "coding tools action requires a CLI prerequisite"
            );
            (false, false)
        }
        Operation::PrepareLockedImage => {
            anyhow::ensure!(
                proposal.prerequisite_id.starts_with("setup:images:"),
                "image preparation requires an image prerequisite"
            );
            anyhow::ensure!(
                proposal.image_profile.as_deref().is_some_and(|profile| {
                    matches!(profile, "general" | "rust" | "node" | "python" | "go")
                }),
                "only bundled, approved runtime profiles may be prepared"
            );
            (true, true)
        }
    };
    Ok(ReviewedAction {
        operation: proposal.operation,
        prerequisite_id: proposal.prerequisite_id.clone(),
        requires_consent,
        requires_engine_verification,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(operation: Operation, id: &str, image: Option<&str>) -> Proposal {
        Proposal {
            schema_version: 1,
            operation,
            host: Host::Linux,
            prerequisite_id: id.into(),
            image_profile: image.map(str::to_owned),
        }
    }

    #[test]
    fn permits_non_mutating_verification_without_host_shell() {
        let item = action(Operation::RecheckPrerequisites, "setup:docker:rootless", None);
        let reviewed = validate(&item, Host::Linux).unwrap();
        assert!(!reviewed.requires_consent);
    }

    #[test]
    fn image_operation_requires_specific_approval_and_engine_verification() {
        let item = action(Operation::PrepareLockedImage, "setup:images:general:amd64", Some("general"));
        let reviewed = validate(&item, Host::Linux).unwrap();
        assert!(reviewed.requires_consent);
        assert!(reviewed.requires_engine_verification);
    }

    #[test]
    fn rejects_unapproved_registry_references_and_mutating_host_commands() {
        assert!(validate(&action(Operation::PrepareLockedImage, "setup:images:general:amd64", Some("example.com/evil:latest")), Host::Linux).is_err());
        assert!(serde_json::from_str::<Proposal>(
            r#"{"schemaVersion":1,"operation":"run_host_command","host":"linux","prerequisiteId":"setup:docker:rootless","imageProfile":null}"#
        ).is_err());
        assert!(serde_json::from_str::<Proposal>(
            r#"{"schemaVersion":1,"operation":"recheck_prerequisites","host":"linux","prerequisiteId":"setup:docker:rootless","imageProfile":null,"shell":"sudo anything"}"#
        ).is_err());
    }

    #[test]
    fn rejects_host_identity_or_domain_changes() {
        let p = action(Operation::OpenCodingTools, "setup:cli:codex:auth", None);
        assert!(validate(&p, Host::Windows).is_err());
        assert!(validate(&action(Operation::OpenCodingTools, "setup:docker:start", None), Host::Linux).is_err());
        assert!(validate(&action(Operation::RecheckPrerequisites, "setup:../../escape", None), Host::Linux).is_err());
    }
}
