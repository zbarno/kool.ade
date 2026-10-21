use std::{fs, process::Command, sync::Arc, time::Duration};

use crate::execution::{
    contract::{ExecutionModeV1, ExecutionTarget},
    docker::{
        image_extension::{
            LinuxDistribution, PackageTrust, SystemImageExtensionRequest,
            builder::{PackageArtifactIdentity, VerifiedPackageArtifact},
        },
        images::{ImageLock, RuntimeProfile},
    },
    session::ExecutionSession,
};
use sha2::{Digest, Sha256};

#[path = "system_image/broker_prerequisite.rs"]
mod broker_prerequisite;

use super::support::{Fixture, OPT_IN, backend, native_docker_arch, request, workspace};

/// Production boundary: controller authorization -> local Docker build API -> pinned image ID ->
/// isolated task container. Invariant: only the generated, hash-checked package context is built;
/// Docker build and the later worker both have networking disabled and no host mounts/secrets.
/// A unit test cannot prove the selected Docker daemon actually enforces these options or that the
/// package is installed and the resulting digest is selected by workspace startup. Requires a
/// native Linux Docker daemon and `dpkg-deb`; the trusted base image must be cached or pullable.
/// Setup cost is moderate (first base-image pull/build can take several minutes).
#[test]
fn authorized_system_image_build_is_offline_pinned_and_selected() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping Docker Engine integration; set {OPT_IN}=1 to opt in");
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 currently requires a native Linux Docker Engine");
    }

    let fixture = Fixture::new();
    let task = workspace();
    let (engine, _) = backend(&fixture, task.clone());
    let engine = Arc::new(engine);
    let package_bytes = build_synthetic_deb(&fixture.root);
    let identity = PackageArtifactIdentity {
        name: "koolade-issue137-test".into(),
        version: "1.0-1".into(),
        architecture: native_docker_arch().into(),
        source: format!(
            "https://deb.debian.org/debian/pool/main/k/koolade-issue137-test/koolade-issue137-test_1.0-1_{}.deb",
            native_docker_arch()
        ),
        sha256: format!("{:x}", Sha256::digest(&package_bytes)),
    };
    let base = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, native_docker_arch())
        .unwrap();
    let base_digest = base.digest.clone();
    let proposal = SystemImageExtensionRequest {
        task_id: task.task_id.clone(),
        project_id: task.project_id.clone(),
        distribution: LinuxDistribution::DebianBookworm,
        base_profile: RuntimeProfile::General,
        base_image_digest: base.digest,
        target_arch: native_docker_arch().into(),
        packages: vec![identity.name.clone()],
        trust: PackageTrust::ApprovedDistributionRepository,
        purpose: "Test isolated image derivation".into(),
    };
    let authorized = proposal
        .authorize(
            crate::harness::DependencyDecision::UserAuthorizeForTask,
            &task.task_id,
            vec![identity.clone()],
        )
        .unwrap();
    let seed_request = request(task.clone(), "");
    let session = ExecutionSession::new(
        engine,
        task,
        ExecutionTarget::local(),
        ExecutionModeV1::Implementation,
        base_digest,
        seed_request.resource_budget,
    )
    .unwrap();
    let derived = session
        .derive_authorized_system_image(
            authorized,
            vec![VerifiedPackageArtifact {
                identity,
                bytes: package_bytes,
            }],
        )
        .expect("authorized image must build and verify offline");
    assert!(derived.image_id.starts_with("sha256:"));

    let script = "set -eu\ntest \"$(dpkg-query -W -f='${Version}' koolade-issue137-test)\" = 1.0-1\nroute_lines=$(wc -l < /proc/net/route)\ntest \"$route_lines\" -eq 1\nprintf 'derived-image-ready\\n'\n";
    let output = session
        .run_tool_command(script, Duration::from_secs(30))
        .expect("session must select and run the pinned derived digest");
    assert_eq!(output.result.exit_code, Some(0));
    assert!(output.stdout.contains("derived-image-ready"));
}

/// Production proof: verify Debian's signed Release and index, resolve the
/// approved closure against the exact base image, fetch and hash package bytes,
/// then build with Docker networking disabled and select the returned digest.
/// It requires HTTPS access to `deb.debian.org`, Linux Docker and the pinned
/// base image; first run has high setup cost due to the compressed package index
/// and base-image pull. The sibling test checks the worker's route table.
#[test]
fn signed_bookworm_closure_builds_through_offline_docker_control_plane() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 currently requires a native Linux Docker Engine");
    }

    let fixture = Fixture::new();
    let task = workspace();
    let (engine, _) = backend(&fixture, task.clone());
    let base = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, native_docker_arch())
        .unwrap();
    let base_digest = base.digest.clone();
    let seed = request(task.clone(), "");
    let session = ExecutionSession::new(
        Arc::new(engine),
        task,
        ExecutionTarget::local(),
        ExecutionModeV1::Implementation,
        base_digest,
        seed.resource_budget,
    )
    .unwrap();
    let mut need = crate::harness::DependencyNeed {
        ecosystem: crate::harness::PackageEcosystem::System,
        package: Some("hello".into()),
        version: None,
        source: Some("debian:bookworm".into()),
        command: "apt-get install -y hello".into(),
        reason: "Run the approved greeting utility for this task".into(),
        kind: crate::harness::DependencyKind::SystemTool,
        lockfile_identity: None,
        introduced_packages: Vec::new(),
    };
    need.introduced_packages = session.resolve_system_dependency_manifest(&need).unwrap();
    let hello_version = need
        .introduced_packages
        .iter()
        .find(|package| package.package == "hello")
        .expect("root package in signed closure")
        .version
        .clone();
    let derived = session
        .prepare_authorized_system_dependency(
            &need,
            crate::harness::DependencyDecision::UserAuthorizeForTask,
        )
        .expect("explicitly approved signed package snapshot builds offline");
    assert!(derived.image_id.starts_with("sha256:"));
    let output = session.run_tool_command(&format!(
        "set -eu\ntest \"$(dpkg-query -W -f='${{Version}}' hello)\" = '{hello_version}'\ntest \"$(wc -l < /proc/net/route)\" -eq 1\nprintf 'signed-offline-image-ready\\n'\n"
    ), Duration::from_secs(30)).unwrap();
    assert_eq!(output.result.exit_code, Some(0));
    assert!(output.stdout.contains("signed-offline-image-ready"));
}

fn build_synthetic_deb(root: &std::path::Path) -> Vec<u8> {
    let package = root.join("synthetic-deb");
    let control = package.join("DEBIAN/control");
    fs::create_dir_all(control.parent().unwrap()).unwrap();
    fs::write(
        control,
        format!(
            "Package: koolade-issue137-test\nVersion: 1.0-1\nArchitecture: {}\nMaintainer: Test <test@example.invalid>\nDescription: synthetic image extension test package\n",
            native_docker_arch()
        ),
    )
    .unwrap();
    let output = root.join("koolade-issue137-test.deb");
    let status = Command::new("dpkg-deb")
        .args(["--build"])
        .arg(&package)
        .arg(&output)
        .status()
        .expect("dpkg-deb must be installed for the opt-in Docker test");
    assert!(status.success());
    fs::read(output).unwrap()
}
