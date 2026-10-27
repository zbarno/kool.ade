use super::*;
use crate::execution::docker::images::{ImageLock, RuntimeProfile};
use crate::execution::{
    contract::ReadinessState,
    docker::environment::{DockerEnvironmentState, DockerEnvironmentStatus},
};
use crate::harness::discovery::{
    ExecutionCapability, ModelCatalogStatus, ProbeResult, ProbeStatus,
};

fn docker(state: DockerEnvironmentState, engine_ready: bool) -> DockerEnvironmentStatus {
    DockerEnvironmentStatus {
        state,
        engine_ready,
        message: String::new(),
        desktop_vm: false,
        daemon_version: None,
        architecture: Some("amd64".into()),
        rootless: Some(engine_ready),
        cpu_limits: Some(true),
        memory_limits: Some(true),
        pids_limit: Some(true),
        cgroups_supported: engine_ready.then_some(true),
        available_images: Vec::new(),
        disk_available_bytes: None,
        endpoint_fingerprint: None,
        daemon_identity_fingerprint: engine_ready.then(|| "d".repeat(64)),
    }
}

fn agent(auth: ReadinessState) -> ProbeResult {
    ProbeResult {
        provider_id: "codex".into(),
        binary_path: Some("/test/codex".into()),
        version: Some("1".into()),
        status: ProbeStatus::Available,
        authentication: auth,
        model_catalog_status: ModelCatalogStatus::Fresh,
        models: vec!["gpt".into()],
        execution_capability: ExecutionCapability {
            safe_for_planning: ReadinessState::Ready,
            safe_for_implementation: ReadinessState::Unavailable,
            runtime_ready: ReadinessState::Unavailable,
        },
        ..ProbeResult::default()
    }
}

fn assess_with_images(
    docker: &DockerEnvironmentStatus,
    required_profiles: std::collections::BTreeSet<RuntimeProfile>,
    smoke_verified_profiles: std::collections::BTreeSet<RuntimeProfile>,
    checked_at_unix_ms: u64,
) -> MachineSnapshotV1 {
    assess_for_scan_with_inputs(
        &[],
        None,
        docker,
        HostKind::Linux,
        checked_at_unix_ms,
        1,
        ScanInputs {
            images: ImageScanState {
                required_profiles,
                smoke_verified_profiles,
            },
            ..ScanInputs::default()
        },
    )
}

#[test]
fn rootful_docker_does_not_hide_authenticated_cli_or_invent_image_error() {
    let snapshot = assess(
        &[agent(ReadinessState::Ready)],
        Some("codex"),
        &docker(DockerEnvironmentState::RootfulLinuxRejected, false),
        HostKind::Linux,
        1,
    );
    assert!(
        snapshot
            .findings
            .iter()
            .any(|finding| finding.id == "setup:docker:rootless")
    );
    assert!(
        !snapshot
            .findings
            .iter()
            .any(|finding| finding.id.starts_with("setup:images:"))
    );
}

#[test]
fn missing_image_is_only_a_finding_after_healthy_engine_probe() {
    let snapshot = assess(
        &[agent(ReadinessState::Ready)],
        Some("codex"),
        &docker(DockerEnvironmentState::MissingApprovedImage, true),
        HostKind::Linux,
        2,
    );
    assert_eq!(snapshot.findings.len(), 1);
    assert_eq!(
        snapshot.findings[0].id,
        format!(
            "setup:images:general:amd64:{}",
            docker(DockerEnvironmentState::MissingApprovedImage, true)
                .engine_identity_fingerprint()
                .unwrap()
        )
    );
}

#[test]
fn another_profile_does_not_hide_a_missing_general_image() {
    let lock = ImageLock::bundled().unwrap();
    let rust = lock.select(RuntimeProfile::Rust, "amd64").unwrap();
    let mut engine = docker(DockerEnvironmentState::Ready, true);
    engine.available_images.push(rust.reference);
    let snapshot = assess(&[], None, &engine, HostKind::Linux, 3);
    assert!(snapshot.findings.iter().any(|finding| {
        finding.id
            == format!(
                "setup:images:general:amd64:{}",
                engine.engine_identity_fingerprint().unwrap()
            )
    }));
    assert!(
        snapshot
            .checked_domains
            .iter()
            .all(|domain| !domain.starts_with("images:rust:"))
    );
    assert!(
        snapshot
            .checked_domains
            .iter()
            .all(|domain| !domain.starts_with("images:general:"))
    );
}

#[test]
fn workload_requirement_creates_only_its_missing_runtime_profile_task() {
    let engine = docker(DockerEnvironmentState::MissingApprovedImage, true);
    let snapshot = assess_with_images(
        &engine,
        [RuntimeProfile::Rust].into_iter().collect(),
        std::collections::BTreeSet::new(),
        31,
    );
    let identity = engine.engine_identity_fingerprint().unwrap();
    assert!(
        snapshot
            .findings
            .iter()
            .any(|finding| { finding.id == format!("setup:images:rust:amd64:{identity}") })
    );
    assert!(
        snapshot
            .findings
            .iter()
            .any(|finding| { finding.id == format!("setup:images:general:amd64:{identity}") })
    );
    assert!(!snapshot.findings.iter().any(|finding| {
        finding.id.starts_with("setup:images:node:")
            || finding.id.starts_with("setup:images:python:")
            || finding.id.starts_with("setup:images:go:")
    }));
}

#[test]
fn present_general_image_still_requires_a_current_smoke_receipt() {
    let lock = ImageLock::bundled().unwrap();
    let general = lock.select(RuntimeProfile::General, "amd64").unwrap();
    let mut engine = docker(DockerEnvironmentState::Ready, true);
    engine.available_images.push(general.reference);
    let identity = engine.engine_identity_fingerprint().unwrap();
    let snapshot = assess_with_images(
        &engine,
        std::collections::BTreeSet::new(),
        [RuntimeProfile::General].into_iter().collect(),
        4,
    );
    let image_task = snapshot
        .findings
        .iter()
        .find(|finding| finding.id == format!("setup:images:general:amd64:{identity}"))
        .expect("present but un-smoked image has a guided smoke task");
    assert!(
        !image_task
            .prerequisites
            .iter()
            .any(|id| id == "setup:docker:rootless"),
        "the rootless summary waits for smoke and cannot gate the smoke task"
    );
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain.starts_with("images:general:amd64:")),
        "inventory alone does not verify the approved image smoke task"
    );
    assert!(
        !snapshot
            .checked_domains
            .iter()
            .any(|domain| domain == "docker:rootless-smoke"),
        "the smoke domain requires a current persisted receipt"
    );
    assert!(
        !snapshot
            .findings
            .iter()
            .any(|finding| finding.id == "setup:images:general:unverified")
    );
}

#[test]
fn unknown_authentication_is_distinct_from_installation_and_docker_failure() {
    let snapshot = assess(
        &[agent(ReadinessState::Unknown)],
        Some("codex"),
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Windows,
        3,
    );
    assert!(
        snapshot
            .findings
            .iter()
            .any(|f| f.id == "setup:cli:codex:verify")
    );
    assert!(
        snapshot
            .findings
            .iter()
            .any(|f| f.id == "setup:docker:install")
    );
    assert!(
        snapshot
            .findings
            .iter()
            .any(|f| f.id == "setup:docker:desktop-transport")
    );
    assert!(
        !snapshot
            .findings
            .iter()
            .any(|f| f.id.starts_with("setup:images"))
    );
}

#[test]
fn no_cli_produces_one_choice_not_six_install_tasks() {
    let snap = assess(
        &[],
        None,
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        4,
    );
    assert!(snap.findings.iter().any(|f| f.id == "setup:cli:install"));
    assert_eq!(
        snap.findings
            .iter()
            .filter(|finding| finding.id.starts_with("setup:cli:"))
            .count(),
        1
    );
    assert!(
        snap.findings
            .iter()
            .any(|f| f.id == "setup:docker:rootless")
    );
    assert!(!snap.findings.iter().any(|f| f.id == "setup:docker:install"));
}

#[test]
fn selected_agent_and_mac_desktop_both_have_independent_readiness() {
    let mut engine = docker(DockerEnvironmentState::Ready, true);
    engine.available_images.push("general".into());
    let snap = assess(
        &[agent(ReadinessState::Ready)],
        Some("codex"),
        &engine,
        HostKind::MacOs,
        5,
    );
    assert_eq!(snap.findings.len(), 1);
    assert_eq!(snap.findings[0].status, FindingStatus::Unsupported);
}

mod validation;
