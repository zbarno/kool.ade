use super::{
    EvidenceCode, EvidenceOrigin, EvidenceState, HostKind, PrerequisiteDomain, PrerequisiteSubject,
    ProbeEvidenceV1, ProbeObservationV1,
};

fn qemu_version_is_valid(version: &str) -> bool {
    if version.len() > 160 || version.chars().any(char::is_control) {
        return false;
    }
    let Some(version) = version.strip_prefix("QEMU emulator version ") else {
        return false;
    };
    let version = version.split_ascii_whitespace().next().unwrap_or_default();
    let mut parts = version.split('.');
    let numeric = [parts.next(), parts.next(), parts.next()];
    numeric.into_iter().all(|part| {
        part.is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && parts.next().is_none()
}

pub(in crate::app::setup_attention::catalog::model::snapshot::validation) fn linux_qemu_evidence_is_consistent(
    item: &ProbeEvidenceV1,
) -> bool {
    if item.prerequisite.domain != PrerequisiteDomain::Host
        || item.prerequisite.platform != HostKind::Linux
        || item.prerequisite.subject != PrerequisiteSubject::None
        || item.origin != EvidenceOrigin::HostRuntime
        || item.source_generation.is_some()
    {
        return false;
    }
    let Some(ProbeObservationV1::LinuxQemu {
        version,
        kvm_acceleration,
        qemu_image_tool_available,
        qemu_launcher_contained,
    }) = item.observation.as_ref()
    else {
        return false;
    };
    match (item.state, item.code, version, kvm_acceleration) {
        (EvidenceState::Ready, EvidenceCode::LinuxQemuDetected, Some(version), Some(_)) => {
            qemu_launcher_contained == &Some(true) && qemu_version_is_valid(version)
        }
        (EvidenceState::Missing, EvidenceCode::LinuxQemuMissing, None, None)
            if qemu_image_tool_available.is_none() =>
        {
            true
        }
        (EvidenceState::Failed, EvidenceCode::LinuxQemuPermissionDenied, None, None)
        | (EvidenceState::Unknown, EvidenceCode::LinuxQemuUnknown, None, None)
        | (EvidenceState::Unsupported, EvidenceCode::LinuxProbeUnsupported, None, None)
            if qemu_image_tool_available.is_none() =>
        {
            true
        }
        _ => false,
    }
}
