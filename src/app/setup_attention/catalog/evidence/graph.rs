use super::id;
use crate::app::setup_attention::catalog::{
    EvidenceState as State, HostKind, PrerequisiteCapability as Capability,
    PrerequisiteDomain as Domain, PrerequisiteId, PrerequisiteNodeV1 as Node,
    PrerequisiteSubject as Subject, ProbeEvidenceV1 as Evidence, RemediationEligibility,
};

fn dependencies(item: &Evidence, host: HostKind) -> Vec<PrerequisiteId> {
    let prerequisite = &item.prerequisite;
    let none = Subject::None;
    let desktop_host = matches!(host, HostKind::MacOs | HostKind::Windows);
    match prerequisite.capability {
        Capability::ProviderVersion
        | Capability::ProviderAuthentication
        | Capability::ProviderModels
        | Capability::ProviderContextOnly => vec![id(
            Domain::Provider,
            host,
            Capability::ProviderExecutable,
            prerequisite.subject.clone(),
        )],
        Capability::GitIdentity => vec![id(
            Domain::Git,
            host,
            Capability::GitExecutable,
            none.clone(),
        )],
        Capability::WindowsWslFeature | Capability::WindowsVmPlatformFeature => vec![
            id(Domain::Host, host, Capability::HostEdition, none.clone()),
            id(
                Domain::Host,
                host,
                Capability::WindowsWslPolicy,
                none.clone(),
            ),
        ],
        Capability::WindowsWslKernel => vec![
            id(
                Domain::Host,
                host,
                Capability::WindowsWslFeature,
                none.clone(),
            ),
            id(
                Domain::Host,
                host,
                Capability::WindowsWslPolicy,
                none.clone(),
            ),
        ],
        Capability::WindowsWslPolicy | Capability::WindowsReboot => Vec::new(),
        Capability::Wsl2 => [
            Capability::HostEdition,
            Capability::WindowsWslFeature,
            Capability::WindowsVmPlatformFeature,
            Capability::WindowsWslKernel,
            Capability::WindowsReboot,
            Capability::WindowsWslPolicy,
            Capability::Virtualization,
        ]
        .into_iter()
        .map(|capability| id(Domain::Host, host, capability, none.clone()))
        .collect(),
        Capability::DockerCgroups => vec![id(
            Domain::Docker,
            host,
            Capability::DockerEngine,
            none.clone(),
        )],
        Capability::ApprovedImage => {
            let mut dependencies = vec![
                id(Domain::Docker, host, Capability::DockerEngine, none.clone()),
                id(
                    Domain::Docker,
                    host,
                    Capability::DockerEndpoint,
                    none.clone(),
                ),
            ];
            if desktop_host {
                dependencies.push(id(Domain::Host, host, Capability::DockerDesktopTrust, none));
            }
            dependencies
        }
        Capability::DockerRootlessSmoke => {
            let mut dependencies = vec![
                id(Domain::Docker, host, Capability::DockerEngine, none.clone()),
                id(
                    Domain::Docker,
                    host,
                    Capability::DockerEndpoint,
                    none.clone(),
                ),
            ];
            if matches!(&prerequisite.subject, Subject::Image { .. }) {
                dependencies.push(id(
                    Domain::Image,
                    host,
                    Capability::ApprovedImage,
                    prerequisite.subject.clone(),
                ));
            }
            dependencies
        }
        Capability::DockerEngine | Capability::DockerEndpoint if desktop_host => {
            vec![id(Domain::Host, host, Capability::DockerDesktopTrust, none)]
        }
        Capability::DockerSecurity if desktop_host => vec![
            id(Domain::Docker, host, Capability::DockerEngine, none.clone()),
            id(Domain::Host, host, Capability::DockerDesktopTrust, none),
        ],
        Capability::DockerCapacity | Capability::DockerSecurity => {
            vec![id(Domain::Docker, host, Capability::DockerEngine, none)]
        }
        _ => Vec::new(),
    }
}

fn remediation(
    item: &Evidence,
    parents: &[PrerequisiteId],
    evidence: &[Evidence],
) -> RemediationEligibility {
    let blocked = parents.iter().any(|dependency| {
        evidence
            .iter()
            .find(|parent| &parent.prerequisite == dependency)
            .is_none_or(|parent| {
                parent.state != State::Ready && parent.state != State::NotApplicable
            })
    });
    if blocked || item.state == State::Blocked {
        return RemediationEligibility::BlockedByPrerequisite;
    }
    match item.state {
        State::Missing => RemediationEligibility::Eligible,
        State::Ready | State::NotApplicable => RemediationEligibility::NotRequired,
        State::Unknown | State::Stale | State::Failed | State::Unsupported => {
            RemediationEligibility::Unverified
        }
        State::Blocked => RemediationEligibility::BlockedByPrerequisite,
    }
}

pub(super) fn build(evidence: &[Evidence], host: HostKind) -> Vec<Node> {
    evidence
        .iter()
        .map(|item| {
            let depends_on = dependencies(item, host);
            Node {
                id: item.prerequisite.clone(),
                state: item.state,
                remediation: remediation(item, &depends_on, evidence),
                depends_on,
            }
        })
        .collect()
}
