use super::*;
use crate::app::bootstrap_remediation::plan::ArtifactTrust;
use sha2::{Digest, Sha256};
use std::fs::File;

#[cfg(target_os = "linux")]
use std::io::Read;

use crate::app::{
    bootstrap_remediation::plan::{
        ApprovedPublisher, ArtifactSource, RemediationPlan, TrustedProduct,
    },
    bootstrap_remediation::request::RemediationRequest,
    bootstrap_repair_policy::{Host, Operation, Proposal},
};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

struct SignatureVerifier(bool);

impl PublisherSignatureVerifier for SignatureVerifier {
    fn verify(
        &self,
        _file: &File,
        source: &ArtifactSource,
        trust: &ArtifactTrust,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(self.0, "synthetic publisher signature did not match");
        anyhow::ensure!(
            matches!(
                source,
                ArtifactSource::VendorInstaller {
                    publisher: ApprovedPublisher::Docker,
                    product: TrustedProduct::DockerDesktop
                }
            ) && matches!(
                trust,
                ArtifactTrust::PublisherSigned {
                    publisher: ApprovedPublisher::Docker,
                    ..
                }
            ),
            "signature request was not bound to its product and source"
        );
        Ok(())
    }
}

fn vendor_plan(digest: String) -> RemediationPlan {
    let proposal = Proposal {
        schema_version: 1,
        operation: Operation::PrepareLockedImage,
        host: Host::Linux,
        prerequisite_id: format!("setup:images:general:amd64:{}", "a".repeat(64)),
        image_profile: Some("general".into()),
    };
    let identity = crate::app::bootstrap_remediation::identity::HostIdentity::from_facts(
        Host::Linux,
        "6.12",
        "test-machine",
        "test-user",
    )
    .unwrap();
    let request = RemediationRequest::from_proposal_for_test(&proposal, &identity).unwrap();
    let mut plan = RemediationPlan::build_for_test(&request, &identity).unwrap();
    plan.set_artifact_for_test(
        ArtifactSource::VendorInstaller {
            publisher: ApprovedPublisher::Docker,
            product: TrustedProduct::DockerDesktop,
        },
        ArtifactTrust::PublisherSigned {
            sha256: digest,
            publisher: ApprovedPublisher::Docker,
        },
    );
    plan
}

// Production boundary: verifies and pins the artifact file handle consumed by
// installer invocation. Hash-only unit checks cannot catch path replacement
// after verification. Uses synthetic temp files only; no external setup.
#[cfg(any(target_os = "linux", target_os = "windows"))]
#[test]
fn artifact_verification_requires_exact_digest_and_publisher_signature() {
    let directory = tempfile::tempdir().unwrap();
    let private = directory.path().join("private");
    std::fs::create_dir(&private).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let path = private.join("installer.pkg");
    std::fs::write(&path, b"synthetic installer payload").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
    }
    let digest = format!("{:x}", Sha256::digest(b"synthetic installer payload"));
    let plan = vendor_plan(digest.clone());
    let artifact = verify_plan_artifact(&plan, &path, &SignatureVerifier(true)).unwrap();
    assert_eq!(artifact.digest(), digest);
    assert!(verify_plan_artifact(&plan, &path, &SignatureVerifier(false)).is_err());
    artifact.validate().unwrap();
    #[cfg(target_os = "linux")]
    {
        let original = private.join("reviewed-installer.pkg");
        std::fs::rename(&path, &original).unwrap();
        std::fs::write(&path, b"replaced same-user payload").unwrap();
        let mut opened = File::open(artifact.execution_path()).unwrap();
        let mut bytes = Vec::new();
        opened.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"synthetic installer payload");
        artifact.validate().unwrap();
    }
    let wrong_digest_plan = vendor_plan("b".repeat(64));
    assert!(verify_plan_artifact(&wrong_digest_plan, &path, &SignatureVerifier(true)).is_err());
}

// Production boundary: macOS artifact execution stays disabled until an
// immutable handoff adapter exists. This platform-specific test catches an
// accidental enablement that host-neutral unit checks miss; only a temporary
// file is needed, with no external setup.
#[cfg(target_os = "macos")]
#[test]
fn artifact_verification_fails_closed_without_macos_handoff() {
    let directory = tempfile::tempdir().unwrap();
    let private = directory.path().join("private");
    std::fs::create_dir(&private).unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = private.join("installer.pkg");
    std::fs::write(&path, b"synthetic installer payload").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
    let digest = format!("{:x}", Sha256::digest(b"synthetic installer payload"));

    assert!(verify_plan_artifact(&vendor_plan(digest), &path, &SignatureVerifier(true)).is_err());
}

#[cfg(unix)]
// Production boundary: refuses symlinked installer artifacts before opening.
// This covers OS path handling that a pure digest unit test cannot prove; it
// uses a temporary directory and no external dependency.
#[test]
fn artifact_verification_rejects_symlinks() {
    use std::os::unix::fs::symlink;

    let directory = tempfile::tempdir().unwrap();
    let private = directory.path().join("private");
    std::fs::create_dir(&private).unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    let source = private.join("source.pkg");
    std::fs::write(&source, b"synthetic installer payload").unwrap();
    let link = private.join("installer.pkg");
    symlink(&source, &link).unwrap();
    let digest = format!("{:x}", Sha256::digest(b"synthetic installer payload"));
    assert!(verify_plan_artifact(&vendor_plan(digest), &link, &SignatureVerifier(true)).is_err());
}
