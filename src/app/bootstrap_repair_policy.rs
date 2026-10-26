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
    /// Install one exact, app-owned Claude Code release for the current user.
    InstallClaudeCode,
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

pub(crate) fn parse_locked_image_task_id(id: &str) -> Option<(&str, &str, &str)> {
    let mut parts = id.split(':');
    if parts.next()? != "setup" || parts.next()? != "images" {
        return None;
    }
    let profile = parts.next()?;
    let architecture = parts.next()?;
    let engine_identity = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if !matches!(profile, "general" | "rust" | "node" | "python" | "go")
        || !matches!(architecture, "amd64" | "arm64")
        || engine_identity.len() != 64
        || !engine_identity
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    Some((profile, architecture, engine_identity))
}

/// Validate intent only. No host process, Docker action, or permission grant
/// occurs here. An executable remediation engine must enforce this again
/// against fresh host evidence and a separate, real user approval.
pub fn validate(proposal: &Proposal, detected_host: Host) -> anyhow::Result<ReviewedAction> {
    anyhow::ensure!(
        proposal.schema_version == 1,
        "unsupported repair proposal version"
    );
    anyhow::ensure!(
        proposal.host == detected_host,
        "repair proposal host changed"
    );
    anyhow::ensure!(
        proposal.prerequisite_id.starts_with("setup:")
            && proposal.prerequisite_id.len() <= 180
            && proposal
                .prerequisite_id
                .bytes()
                .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_') }),
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
            let profile = proposal
                .image_profile
                .as_deref()
                .filter(|profile| matches!(*profile, "general" | "rust" | "node" | "python" | "go"))
                .ok_or_else(|| {
                    anyhow::anyhow!("only bundled, approved runtime profiles may be prepared")
                })?;
            let image_task = parse_locked_image_task_id(&proposal.prerequisite_id);
            anyhow::ensure!(
                image_task.is_some_and(|(task_profile, _, _)| task_profile == profile),
                "image preparation requires a canonical approved profile, architecture and engine identity"
            );
            (true, true)
        }
        Operation::InstallClaudeCode => {
            anyhow::ensure!(proposal.image_profile.is_none(), "unexpected image profile");
            anyhow::ensure!(
                proposal.prerequisite_id == "setup:cli:claude:install",
                "Claude Code installation requires its canonical prerequisite"
            );
            anyhow::ensure!(
                proposal.host == Host::Linux,
                "Claude Code installation is only reviewed for Linux"
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

    const ENGINE_IDENTITY: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn image_task(profile: &str, architecture: &str) -> String {
        format!("setup:images:{profile}:{architecture}:{ENGINE_IDENTITY}")
    }

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
        let item = action(
            Operation::RecheckPrerequisites,
            "setup:docker:rootless",
            None,
        );
        let reviewed = validate(&item, Host::Linux).unwrap();
        assert!(!reviewed.requires_consent);
    }

    #[test]
    fn image_operation_requires_specific_approval_and_engine_verification() {
        let item = action(
            Operation::PrepareLockedImage,
            &image_task("general", "amd64"),
            Some("general"),
        );
        let reviewed = validate(&item, Host::Linux).unwrap();
        assert!(reviewed.requires_consent);
        assert!(reviewed.requires_engine_verification);
    }

    #[test]
    fn rejects_unapproved_registry_references_and_mutating_host_commands() {
        assert!(
            validate(
                &action(
                    Operation::PrepareLockedImage,
                    &image_task("general", "amd64"),
                    Some("example.com/evil:latest")
                ),
                Host::Linux
            )
            .is_err()
        );
        let install = action(
            Operation::InstallClaudeCode,
            "setup:cli:claude:install",
            None,
        );
        assert!(validate(&install, Host::Linux).unwrap().requires_consent);
        assert!(validate(&install, Host::MacOs).is_err());
        assert!(
            validate(
                &action(
                    Operation::InstallClaudeCode,
                    "setup:cli:codex:install",
                    None
                ),
                Host::Linux
            )
            .is_err()
        );
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
        assert!(
            validate(
                &action(Operation::OpenCodingTools, "setup:docker:start", None),
                Host::Linux
            )
            .is_err()
        );
        assert!(
            validate(
                &action(Operation::RecheckPrerequisites, "setup:../../escape", None),
                Host::Linux
            )
            .is_err()
        );
    }

    #[test]
    fn image_intent_must_name_its_exact_profile_and_supported_architecture() {
        let mut proposal = action(
            Operation::PrepareLockedImage,
            &image_task("rust", "amd64"),
            Some("general"),
        );
        assert!(validate(&proposal, Host::Linux).is_err());

        proposal.prerequisite_id = image_task("general", "x86_64");
        assert!(validate(&proposal, Host::Linux).is_err());
    }
}
