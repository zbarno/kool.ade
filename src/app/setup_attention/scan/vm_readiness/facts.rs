use super::{Check, VmFacts};
use crate::app::setup_attention::catalog::FindingStatus;

mod collect;
pub(super) use collect::collect;

fn host_distribution_check(
    evidence: &[crate::app::setup_attention::catalog::ProbeEvidenceV1],
    generation: u64,
) -> Check<()> {
    use crate::app::setup_attention::catalog::{
        EvidenceState, HostKind, LinuxDistributionV1, PrerequisiteCapability as Capability,
        PrerequisiteDomain, PrerequisiteSubject, ProbeObservationV1,
    };
    let Some(item) = evidence.iter().find(|item| {
        item.prerequisite.domain == PrerequisiteDomain::Host
            && item.prerequisite.platform == HostKind::Linux
            && item.prerequisite.capability == Capability::LinuxDistribution
            && item.prerequisite.subject == PrerequisiteSubject::None
            && item.scan_generation == generation
            && item.source_generation.is_none()
    }) else {
        return Check::Unknown;
    };
    match (&item.state, &item.observation) {
        (
            EvidenceState::Ready,
            Some(ProbeObservationV1::LinuxDistribution {
                distribution: Some(distribution),
                ..
            }),
        ) if *distribution != LinuxDistributionV1::Other => Check::Ready(()),
        (EvidenceState::Unsupported | EvidenceState::Missing, _) => Check::Unsupported,
        _ => Check::Unknown,
    }
}

pub(super) fn kvm_finding(facts: &VmFacts) -> (FindingStatus, &'static str, &'static str) {
    if facts.architecture == Check::Unsupported
        || facts.kernel == Check::Unsupported
        || facts.kvm == Check::Unsupported
        || facts.nested_virtualization == Check::Unsupported
    {
        return (
            FindingStatus::Unsupported,
            "KVM is supported only on verified x86_64 Linux hosts without nested virtualization.",
            "Use a supported x86_64 Linux host directly. Computer sessions stay blocked on nested or unsupported hosts.",
        );
    }
    if facts.kernel != Check::Ready(()) {
        return (
            FindingStatus::Unverified,
            "The Linux kernel version could not be verified for isolated computer sessions.",
            "Finish any pending operating system update, restart the app, and rescan. Kool.ad/e does not change the host kernel.",
        );
    }
    if facts.user_namespaces == Check::Missing || facts.user_namespaces == Check::Unsupported {
        return (
            FindingStatus::NeedsAttention,
            "Linux user namespaces are disabled; the isolated computer launcher cannot be verified safely.",
            "Ask your administrator to enable user namespaces through the distribution's supported settings, then rescan. The app will not change host policy.",
        );
    }
    if facts.user_namespaces != Check::Ready(()) {
        return (
            FindingStatus::Unverified,
            "Linux user namespace support could not be verified for the isolated computer launcher.",
            "Check your distribution's user namespace policy and rescan. The app will not change host policy.",
        );
    }
    match (&facts.cpu_virtualization, &facts.kvm) {
        (Check::Ready(()), Check::Ready(())) => (
            FindingStatus::Unverified,
            "KVM is accessible, but one or more host prerequisites could not be verified.",
            "Keep computer sessions blocked until every current host prerequisite and the VM launcher are verified, then rescan.",
        ),
        (_, Check::Ready(())) => (
            FindingStatus::Unverified,
            "KVM is accessible, but CPU virtualization support could not be verified.",
            "Check firmware virtualization settings with your device administrator, restart the host if they change, and rescan.",
        ),
        (Check::Ready(()), Check::Missing) => (
            FindingStatus::NeedsAttention,
            "CPU virtualization is reported, but the KVM device is unavailable; CPU flags alone do not prove KVM can start.",
            "Enable hardware virtualization in firmware and restart the host, then rescan. Kool.ad/e will not load kernel modules or change device permissions.",
        ),
        (Check::Missing, Check::Missing) => (
            FindingStatus::NeedsAttention,
            "CPU virtualization flags are not present and the KVM device is unavailable.",
            "Use an x86_64 Linux host with hardware virtualization enabled in firmware, then rescan. CPU flags alone are not sufficient.",
        ),
        (_, Check::Missing) => (
            FindingStatus::NeedsAttention,
            "The KVM device is not available on this host.",
            "Ask your administrator to verify the host's KVM driver and device state, then rescan. The app will not load drivers or alter permissions.",
        ),
        (Check::Ready(()), Check::PermissionDenied) => (
            FindingStatus::NeedsAttention,
            "CPU virtualization is reported, but this user cannot open the KVM device.",
            "Ask your administrator to grant this account access through the distribution's supported device policy, then sign in again and rescan. The app will not change groups or permissions.",
        ),
        (_, Check::PermissionDenied) => (
            FindingStatus::NeedsAttention,
            "The KVM device exists, but this user cannot open it.",
            "Ask your administrator to review this account's access to the KVM device, then sign in again and rescan. The app will not change groups or permissions.",
        ),
        (Check::Ready(()), Check::Unknown) => (
            FindingStatus::Unverified,
            "CPU virtualization is reported, but KVM availability could not be verified safely.",
            "Ask your administrator to check KVM device availability and rescan. Do not change device permissions to bypass this check.",
        ),
        (_, Check::Unknown) => (
            FindingStatus::Unverified,
            "KVM availability could not be verified safely.",
            "Check host virtualization and KVM device availability with your administrator, then rescan. Computer sessions stay blocked until verified.",
        ),
        (_, Check::Unsupported) => (
            FindingStatus::Unsupported,
            "KVM is supported only on verified x86_64 Linux hosts without nested virtualization.",
            "Use a supported x86_64 Linux host directly. Computer sessions stay blocked on nested or unsupported hosts.",
        ),
    }
}

fn host_evidence_check(
    evidence: &[crate::app::setup_attention::catalog::ProbeEvidenceV1],
    generation: u64,
    capability: crate::app::setup_attention::catalog::PrerequisiteCapability,
) -> Check<()> {
    use crate::app::setup_attention::catalog::{
        EvidenceState, HostKind, PrerequisiteDomain, PrerequisiteSubject,
    };
    let Some(item) = evidence.iter().find(|item| {
        item.prerequisite.domain == PrerequisiteDomain::Host
            && item.prerequisite.platform == HostKind::Linux
            && item.prerequisite.capability == capability
            && item.prerequisite.subject == PrerequisiteSubject::None
            && item.scan_generation == generation
            && item.source_generation.is_none()
    }) else {
        return Check::Unknown;
    };
    match item.state {
        EvidenceState::Ready => Check::Ready(()),
        EvidenceState::Missing | EvidenceState::Unsupported => Check::Missing,
        EvidenceState::Unknown | EvidenceState::Stale | EvidenceState::Failed => Check::Unknown,
        EvidenceState::Blocked | EvidenceState::NotApplicable => Check::Unsupported,
    }
}

fn host_user_namespaces_check(
    evidence: &[crate::app::setup_attention::catalog::ProbeEvidenceV1],
    generation: u64,
) -> Check<()> {
    use crate::app::setup_attention::catalog::{
        EvidenceState, HostKind, PrerequisiteCapability as Capability, PrerequisiteDomain,
        PrerequisiteSubject, ProbeObservationV1,
    };
    let Some(item) = evidence.iter().find(|item| {
        item.prerequisite.domain == PrerequisiteDomain::Host
            && item.prerequisite.platform == HostKind::Linux
            && item.prerequisite.capability == Capability::LinuxUserNamespaces
            && item.prerequisite.subject == PrerequisiteSubject::None
            && item.scan_generation == generation
            && item.source_generation.is_none()
    }) else {
        return Check::Unknown;
    };
    match (&item.state, &item.observation) {
        (
            EvidenceState::Ready,
            Some(ProbeObservationV1::LinuxUserNamespaces {
                max_user_namespaces: Some(limit),
                unprivileged_clone_allowed,
                apparmor_userns_restricted,
            }),
        ) if *limit > 0
            && *unprivileged_clone_allowed != Some(false)
            && *apparmor_userns_restricted != Some(true) =>
        {
            Check::Ready(())
        }
        (EvidenceState::Unsupported | EvidenceState::Missing, _) => Check::Missing,
        (EvidenceState::Ready | EvidenceState::Unknown | EvidenceState::Failed, _) => {
            Check::Unknown
        }
        _ => Check::Unsupported,
    }
}

#[cfg(test)]
#[path = "facts_tests.rs"]
mod tests;
