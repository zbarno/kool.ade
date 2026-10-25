use super::*;
use crate::{
    app::setup_attention::catalog::EvidenceCode,
    execution::docker::discovery::{PrerequisiteCode, PrerequisiteFailure},
};

#[test]
fn docker_failures_keep_distinct_typed_evidence() {
    for (failure, expected_state, expected_code) in [
        (
            PrerequisiteCode::WindowsContainers,
            EvidenceState::Unsupported,
            EvidenceCode::DockerWindowsContainers,
        ),
        (
            PrerequisiteCode::InsufficientDisk,
            EvidenceState::Failed,
            EvidenceCode::DockerDiskUnavailable,
        ),
        (
            PrerequisiteCode::LimitsUnavailable,
            EvidenceState::Failed,
            EvidenceCode::DockerLimitsUnavailable,
        ),
        (
            PrerequisiteCode::CgroupsUnavailable,
            EvidenceState::Unsupported,
            EvidenceCode::DockerCgroupsUnavailable,
        ),
        (
            PrerequisiteCode::SeccompUnavailable,
            EvidenceState::Failed,
            EvidenceCode::DockerSeccompUnavailable,
        ),
        (
            PrerequisiteCode::UnsupportedApi,
            EvidenceState::Unsupported,
            EvidenceCode::DockerApiUnsupported,
        ),
        (
            PrerequisiteCode::UnsupportedOs,
            EvidenceState::Unsupported,
            EvidenceCode::DockerOsUnsupported,
        ),
        (
            PrerequisiteCode::UnsupportedArchitecture,
            EvidenceState::Unsupported,
            EvidenceCode::DockerArchitectureUnsupported,
        ),
        (
            PrerequisiteCode::WorkspaceUnavailable,
            EvidenceState::Failed,
            EvidenceCode::DockerWorkspaceUnavailable,
        ),
        (
            PrerequisiteCode::ProfileUnsupported,
            EvidenceState::Unsupported,
            EvidenceCode::DockerProfileUnsupported,
        ),
    ] {
        let status = DockerEnvironmentStatus::from_failure(
            PrerequisiteFailure {
                code: failure,
                message: "synthetic typed failure".into(),
                retryable: false,
            },
            false,
        );
        let snapshot =
            super::super::super::assess_for_scan(&[], None, &status, HostKind::Linux, 400, 18);
        let engine = snapshot
            .probe_evidence
            .iter()
            .find(|evidence| {
                evidence.prerequisite.capability == PrerequisiteCapability::DockerEngine
            })
            .unwrap();
        assert_eq!(engine.state, expected_state, "{failure:?}");
        assert_eq!(engine.code, expected_code, "{failure:?}");
        assert!(snapshot.validate().is_ok(), "{failure:?}");
    }
}

#[test]
fn approved_image_failures_keep_absent_uncheckable_and_mismatch_distinct() {
    for (failure, expected_state, expected_code) in [
        (
            PrerequisiteCode::ImageUnavailable,
            EvidenceState::Unknown,
            EvidenceCode::DockerImageUncheckable,
        ),
        (
            PrerequisiteCode::ImageDigestMismatch,
            EvidenceState::Failed,
            EvidenceCode::DockerImageDigestMismatch,
        ),
        (
            PrerequisiteCode::ImageArchitectureMismatch,
            EvidenceState::Unsupported,
            EvidenceCode::DockerImageArchitectureMismatch,
        ),
    ] {
        let status = DockerEnvironmentStatus::from_failure(
            PrerequisiteFailure {
                code: failure,
                message: "synthetic approved image failure".into(),
                retryable: false,
            },
            false,
        );
        let snapshot =
            super::super::super::assess_for_scan(&[], None, &status, HostKind::Linux, 401, 19);
        let image = snapshot
            .probe_evidence
            .iter()
            .find(|evidence| evidence.prerequisite.domain == PrerequisiteDomain::Image)
            .unwrap();
        assert_eq!(image.state, expected_state, "{failure:?}");
        assert_eq!(image.code, expected_code, "{failure:?}");
    }
}
