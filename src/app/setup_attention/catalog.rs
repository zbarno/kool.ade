//! Platform-neutral, read-only bootstrap prerequisite classification.
//!
//! This module consumes bounded provider/Docker evidence; it never launches
//! commands, pulls an image, trusts a model response or changes host state.

use serde::{Deserialize, Serialize};

use crate::execution::docker::environment::{DockerEnvironmentState, DockerEnvironmentStatus};
use crate::harness::discovery::ProbeResult;

pub const SCHEMA_VERSION: u8 = 1;

mod agents;
mod dependencies;
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

/// Pure, deterministic interpretation. Missing image findings require
/// engine_ready; a rootful or unqueryable engine is NOT an image failure.
pub fn assess(
    probes: &[ProbeResult],
    selected_agent: Option<&str>,
    docker: &DockerEnvironmentStatus,
    host: HostKind,
    checked_at_unix_ms: u64,
) -> MachineSnapshot {
    let mut findings = agents::findings(probes, selected_agent);

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
        DockerEnvironmentState::RootfulLinuxRejected => Some(dependencies::requires(
            Finding::action(
                "setup:docker:rootless",
                "Configure secure Docker",
                "Native Linux Docker is running with elevated daemon privileges.",
                "Configure the current user's rootless Docker Engine and verify its socket.",
            ),
            [dependencies::docker_engine()],
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
        DockerEnvironmentState::CapabilitiesIncompatible => Some(dependencies::requires(
            Finding::unverified(
                "setup:docker:capabilities",
                "Resolve Docker compatibility",
                "The Docker engine did not pass the required security or resource checks.",
                "Review the specific Engine diagnostic before changing settings.",
            ),
            [
                dependencies::docker_engine(),
                dependencies::docker_rootless(),
            ],
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
            let mut domains = agents::checked_domains(probes, selected_agent);
            let engine_contacted = docker.engine_ready
                || matches!(
                    docker.state,
                    DockerEnvironmentState::RootfulLinuxRejected
                        | DockerEnvironmentState::CapabilitiesIncompatible
                );
            if docker.engine_ready {
                domains.push("docker".into());
            }
            if engine_contacted {
                domains.push("docker:engine".into());
            }
            if host == HostKind::Linux
                && engine_contacted
                && !docker.desktop_vm
                && docker.rootless == Some(true)
            {
                domains.push("docker:rootless".into());
            }
            if matches!(
                docker.state,
                DockerEnvironmentState::MissingApprovedImage | DockerEnvironmentState::Ready
            ) {
                domains.push("docker:capabilities".into());
            }
            domains.extend(images::checked_domains(docker, host));
            domains
        },
    }
}
