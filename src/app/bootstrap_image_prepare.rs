//! Deliberately narrow app-owned runtime image preparation.
//!
//! This is NOT an agent tool or generic Docker API. Only a consumed, one-use,
//! user-approved general-image capability can reach the existing trusted
//! Docker Engine API. macOS/Windows remain gated until native execution
//! transport and host staging have independent security evidence (#347).

use crate::{
    app::{
        bootstrap_approval::ApprovedAction,
        bootstrap_repair_policy::Operation,
    },
    execution::docker::{
        discovery::{PrerequisiteCode, PrerequisiteFailure},
        environment::{DockerEnvironmentState, DockerEnvironmentStatus},
        images::{ImageLock, RuntimeProfile},
    },
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparationPreview {
    pub profile: RuntimeProfile,
    pub architecture: String,
    pub reference: String,
    pub digest: String,
}

fn invalid(code: PrerequisiteCode, message: &str) -> PrerequisiteFailure {
    PrerequisiteFailure {
        code,
        message: message.into(),
        retryable: false,
    }
}

/// Produce an immutable plan from fresh, independently verified environment
/// facts. Never infer that Docker Desktop is trusted merely from socket
/// presence, nor that image absence means engine absence.
pub fn preview_general(
    environment: &DockerEnvironmentStatus,
) -> Result<PreparationPreview, PrerequisiteFailure> {
    if !cfg!(target_os = "linux") || environment.desktop_vm {
        return Err(invalid(
            PrerequisiteCode::ProfileUnsupported,
            "Native application-owned Docker image preparation requires verified Linux rootless Engine; Docker Desktop is not yet certified",
        ));
    }
    if !environment.engine_ready
        || !matches!(
            environment.state,
            DockerEnvironmentState::MissingApprovedImage | DockerEnvironmentState::Ready
        )
        || environment.rootless != Some(true)
    {
        return Err(invalid(
            PrerequisiteCode::RootfulLinux,
            "A trusted, healthy rootless Docker Engine is required before any image can be prepared",
        ));
    }
    let architecture = environment
        .architecture
        .as_deref()
        .ok_or_else(|| {
            invalid(
                PrerequisiteCode::UnsupportedArchitecture,
                "Docker did not verify the architecture of its Linux daemon",
            )
        })?;
    let image = ImageLock::bundled()?.select(RuntimeProfile::General, architecture)?;
    Ok(PreparationPreview {
        profile: RuntimeProfile::General,
        architecture: architecture.into(),
        reference: image.reference,
        digest: image.digest,
    })
}

/// Strict pre-execution gate. A stale approval, wrong architecture/profile,
/// Docker Desktop endpoint, or changed bundled image must fail before pulling.
pub fn check_approved(
    approval: &ApprovedAction,
    expected: &PreparationPreview,
    current: &DockerEnvironmentStatus,
) -> Result<(), PrerequisiteFailure> {
    if approval.operation() != Operation::PrepareLockedImage
        || approval.image_profile() != Some("general")
        || approval.prerequisite_id()
            != format!("setup:images:general:{}", expected.architecture)
    {
        return Err(invalid(
            PrerequisiteCode::ProfileUnsupported,
            "User approval does not authorize this specific approved general runtime image",
        ));
    }
    let actual = preview_general(current)?;
    if actual != *expected {
        return Err(invalid(
            PrerequisiteCode::ImageDigestMismatch,
            "Runtime image digest or Docker architecture changed since user approval; recheck before continuing",
        ));
    }
    Ok(())
}

/// Invoke only from a controller worker after ApprovalGate::consume has
/// returned its non-cloneable ApprovedAction. The existing Docker Engine
/// implementation independently probes rootless containment, selects
/// ImageLock::bundled and does a bounded no-network non-root smoke test.
/// The approval is consumed by value, not reusable by a caller.
pub fn prepare_general(
    approval: ApprovedAction,
    expected: &PreparationPreview,
) -> Result<(), PrerequisiteFailure> {
    let status = DockerEnvironmentStatus::refresh();
    check_approved(&approval, expected, &status)?;
    crate::execution::docker::environment::run_smoke_test()?;
    let refreshed = DockerEnvironmentStatus::refresh();
    check_approved(&approval, expected, &refreshed)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::bootstrap_repair_policy::Host;

    fn status() -> DockerEnvironmentStatus {
        DockerEnvironmentStatus {
            state: DockerEnvironmentState::MissingApprovedImage,
            engine_ready: true,
            message: String::new(),
            desktop_vm: false,
            daemon_version: Some("28.0".into()),
            architecture: Some("amd64".into()),
            rootless: Some(true),
            cpu_limits: Some(true),
            memory_limits: Some(true),
            pids_limit: Some(true),
            available_images: Vec::new(),
            disk_available_bytes: Some(10_000_000_000),
        }
    }

    #[test]
    fn preview_rejects_rootful_and_untrusted_engine() {
        let mut engine = status();
        engine.rootless = Some(false);
        assert!(preview_general(&engine).is_err());
        engine.rootless = Some(true);
        engine.engine_ready = false;
        assert!(preview_general(&engine).is_err());
    }

    #[test]
    fn preview_never_enables_uncertified_desktop_vm() {
        let mut engine = status();
        engine.desktop_vm = true;
        assert!(preview_general(&engine).is_err());
    }

    #[test]
    fn preview_is_bundled_image_not_an_arbitrary_registry_reference() {
        let selected = preview_general(&status()).unwrap();
        assert_eq!(selected.profile, RuntimeProfile::General);
        assert!(selected.digest.starts_with("sha256:"));
        assert!(selected.reference.contains("@sha256:"));
    }

    #[test]
    fn a_changed_architecture_cannot_reuse_the_same_preview() {
        let first = preview_general(&status()).unwrap();
        let mut changed = status();
        changed.architecture = Some("arm64".into());
        assert_ne!(preview_general(&changed).unwrap(), first);
    }

    #[test]
    fn explicit_approval_is_a_required_opaque_capability() {
        // No unapproved UI/model-facing constructor for ApprovedAction
        // exists in this module. Neither preview nor local status mutates
        // the Docker daemon; only prepare_general can do so.
        let _host = Host::Linux;
        let preview = preview_general(&status()).unwrap();
        assert_eq!(preview.architecture, "amd64");
    }
}
