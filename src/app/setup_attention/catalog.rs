//! Platform-neutral, read-only bootstrap prerequisite classification.
//!
//! This module consumes bounded provider/Docker evidence; it never launches
//! commands, pulls an image, trusts a model response or changes host state.

use serde::{Deserialize, Serialize};

use crate::execution::{
    contract::ReadinessState,
    docker::environment::{DockerEnvironmentState, DockerEnvironmentStatus},
};
use crate::harness::discovery::{ModelCatalogStatus, ProbeResult, ProbeStatus};

pub const SCHEMA_VERSION: u8 = 1;

mod images;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostKind {
    Linux,
    MacOs,
    Windows,
    Unsupported,
}

impl HostKind {
    pub fn current() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Unsupported
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatus {
    NeedsAttention,
    BlockedByPrerequisite,
    Unverified,
    Unsupported,
}

/// A specific fault, not a generic "Docker needs attention" board card.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    pub title: String,
    pub explanation: String,
    pub next_step: String,
    pub status: FindingStatus,
    pub prerequisites: Vec<String>,
}

impl Finding {
    pub(super) fn action(id: impl Into<String>, title: &str, why: &str, next: &str) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            explanation: why.into(),
            next_step: next.into(),
            status: FindingStatus::NeedsAttention,
            prerequisites: Vec::new(),
        }
    }

    pub(super) fn unverified(id: impl Into<String>, title: &str, why: &str, next: &str) -> Self {
        Self {
            status: FindingStatus::Unverified,
            ..Self::action(id, title, why, next)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MachineSnapshot {
    pub schema_version: u8,
    pub host: HostKind,
    pub checked_at_unix_ms: u64,
    pub findings: Vec<Finding>,
    /// Only these domains may have stale tasks marked resolved.
    pub checked_domains: Vec<String>,
}

fn authenticated_context_agent(probe: &ProbeResult) -> bool {
    probe.status == ProbeStatus::Available
        && probe.binary_path.is_some()
        && probe.version.is_some()
        && probe.authentication == ReadinessState::Ready
        && probe.execution_capability.safe_for_planning == ReadinessState::Ready
        && probe.model_catalog_status == ModelCatalogStatus::Fresh
        && (!probe.models.is_empty() || probe.default_model.is_some())
}

/// Pure, deterministic interpretation. Missing image findings require
/// engine_ready; a rootful or unqueryable engine is NOT an image failure.
pub fn assess(
    probes: &[ProbeResult],
    selected_agent: Option<&str>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    checked_at_unix_ms: u64,
) -> MachineSnapshot {
    let mut findings = Vec::new();
    let installed: Vec<_> = probes.iter().filter(|p| p.binary_path.is_some()).collect();
    let verified: Vec<_> = installed
        .iter()
        .copied()
        .filter(|p| authenticated_context_agent(p))
        .collect();

    if installed.is_empty() {
        let crashed = probes.iter().any(|p| p.status == ProbeStatus::Failed);
        findings.push(if crashed {
            Finding::unverified(
                "setup:cli:scan",
                "Check coding agent detection",
                "One of the coding agent checks did not finish reliably.",
                "Retry Coding Tools discovery before choosing an installer.",
            )
        } else {
            Finding::action(
                "setup:cli:install",
                "Choose a coding agent",
                "No supported coding agent is installed. Kool.ad/e can guide setup without one.",
                "Choose an agent in Coding Tools to get its official installation steps.",
            )
        });
    } else {
        // Only the chosen provider (or installed candidates before selection)
        // should create configuration cards, not every absent optional CLI.
        for probe in installed {
            let prefix = format!("setup:cli:{}", probe.provider_id);
            if probe.authentication == ReadinessState::Unavailable {
                findings.push(Finding::action(
                    format!("{prefix}:auth"),
                    "Connect your coding agent account",
                    "The installed coding agent needs an authenticated account.",
                    "Sign in using the agent's official account flow and retry verification.",
                ));
            } else if probe.authentication == ReadinessState::Unknown {
                findings.push(Finding::unverified(
                    format!("{prefix}:verify"),
                    "Verify coding agent access",
                    "This coding agent is installed, but its sign-in status could not be verified.",
                    "Review this agent in Coding Tools; unknown access is not treated as ready.",
                ));
            } else if probe.model_catalog_status != ModelCatalogStatus::Fresh
                || (probe.models.is_empty() && probe.default_model.is_none())
            {
                findings.push(Finding::unverified(
                    format!("{prefix}:models"),
                    "Configure a usable AI model",
                    "This agent's current model availability is not verified.",
                    "Select a supported model and refresh this agent's model catalog.",
                ));
            } else if !authenticated_context_agent(probe) {
                findings.push(Finding::unverified(
                    format!("{prefix}:capability"),
                    "Verify safe agent mode",
                    "The coding agent cannot yet be verified for context-only setup assistance.",
                    "Review its capability diagnostics; host tools remain disabled.",
                ));
            }
        }
        if !verified.is_empty()
            && !selected_agent
                .is_some_and(|id| verified.iter().any(|probe| probe.provider_id == id))
        {
            findings.push(Finding::action(
                "setup:cli:select",
                "Choose your setup assistant",
                "An agent is ready, but none has been explicitly selected.",
                "Select an authenticated agent in Coding Tools.",
            ));
        }
    }

    let docker_blocker = match docker.state {
        DockerEnvironmentState::NotInstalled => Some(Finding::action(
            "setup:docker:install",
            "Install a safe container runtime",
            "Kool.ad/e cannot find a supported local Docker runtime.",
            "Follow the supported Docker installation task for this operating system.",
        )),
        DockerEnvironmentState::NotRunning => Some(Finding::action(
            "setup:docker:start",
            "Start the container service",
            "The Docker service or Linux VM is not running.",
            "Start the verified local container service, then check again.",
        )),
        DockerEnvironmentState::PermissionDenied => Some(Finding::action(
            "setup:docker:permission",
            "Allow access to the container service",
            "Kool.ad/e cannot access the selected local Docker service.",
            "Check the service owner and supported user permissions.",
        )),
        DockerEnvironmentState::RootfulLinuxRejected => Some(Finding::action(
            "setup:docker:rootless",
            "Configure secure Docker",
            "Native Linux Docker is running with elevated daemon privileges.",
            "Configure the current user's rootless Docker Engine and verify its socket.",
        )),
        DockerEnvironmentState::DesktopVmRequired => Some(Finding::action(
            "setup:docker:desktop",
            "Start Docker Desktop's Linux environment",
            "Docker Desktop's local Linux backend is unavailable.",
            "Install or start Docker Desktop with its supported Linux container backend.",
        )),
        DockerEnvironmentState::UntrustedEndpoint => Some(Finding::action(
            "setup:docker:endpoint",
            "Connect to trusted local Docker",
            "The selected Docker endpoint cannot be trusted.",
            "Choose the locally verified Docker Engine or Docker Desktop connection.",
        )),
        DockerEnvironmentState::CapabilitiesIncompatible => Some(Finding::unverified(
            "setup:docker:capabilities",
            "Resolve Docker compatibility",
            "The Docker engine did not pass the required security or resource checks.",
            "Review the specific Engine diagnostic before changing settings.",
        )),
        DockerEnvironmentState::MissingApprovedImage | DockerEnvironmentState::Ready => None,
    };
    if let Some(finding) = docker_blocker {
        findings.push(finding);
    }

    findings.extend(images::findings(docker, host));

    // Docker Desktop endpoints are parseable today, but safe end-to-end
    // execution on these hosts is not production validated (#347).
    if matches!(host, HostKind::MacOs | HostKind::Windows) {
        findings.push(Finding {
            status: FindingStatus::Unsupported,
            ..Finding::unverified(
                "setup:docker:desktop-transport",
                "Verify Docker Desktop compatibility",
                "Docker Desktop execution on this host is not yet release-certified.",
                "Complete native Docker Desktop staging/security validation before enabling coding tasks.",
            )
        });
    } else if host == HostKind::Unsupported {
        findings.push(Finding {
            status: FindingStatus::Unsupported,
            ..Finding::unverified(
                "setup:host:unsupported",
                "Unsupported operating system",
                "Application-owned Docker execution has not been validated on this host.",
                "Use a supported operating system for coding tasks.",
            )
        });
    }

    findings.sort_by(|left, right| left.id.cmp(&right.id));
    findings.dedup_by(|left, right| left.id == right.id);
    MachineSnapshot {
        schema_version: SCHEMA_VERSION,
        host,
        checked_at_unix_ms,
        findings,
        checked_domains: {
            // Readiness, not merely completed probing, can resolve old tasks.
            // A stopped/rootful/untrusted Engine cannot prove that an earlier
            // Docker prerequisite was fixed. Likewise, an unknown CLI account
            // cannot verify a previously failing provider.
            let mut domains = Vec::new();
            if probes.iter().any(|probe| probe.binary_path.is_some()) {
                domains.push("cli:install".into());
            }
            if probes
                .iter()
                .all(|probe| probe.status != ProbeStatus::Failed)
            {
                domains.push("cli:scan".into());
            }
            for probe in &verified {
                domains.push(format!("cli:{}", probe.provider_id));
            }
            if selected_agent.is_some_and(|id| verified.iter().any(|probe| probe.provider_id == id))
            {
                domains.push("cli:select".into());
            }
            if docker.engine_ready {
                domains.push("docker".into());
            }
            domains.extend(images::checked_domains(docker, host));
            domains
        },
    }
}
