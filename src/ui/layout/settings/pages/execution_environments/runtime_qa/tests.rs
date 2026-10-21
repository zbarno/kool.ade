use super::{profile_status, relay_status, reviewer_status};
use crate::{
    execution::{
        contract::{HarnessCapabilitySnapshotV1, HarnessRole, ReadinessState},
        docker::{
            environment::{DockerEnvironmentState, DockerEnvironmentStatus},
            images::{ImageLock, ImageVariant},
        },
    },
    persistence::harness_settings::{DetectedHarness, HarnessSettings, QA, WorkRoute},
};
use std::collections::BTreeMap;

fn docker_ready(available_images: Vec<String>) -> DockerEnvironmentStatus {
    DockerEnvironmentStatus {
        state: DockerEnvironmentState::Ready,
        engine_ready: true,
        message: "ready".into(),
        desktop_vm: false,
        daemon_version: Some("test".into()),
        architecture: Some("amd64".into()),
        rootless: Some(true),
        cpu_limits: Some(true),
        memory_limits: Some(true),
        pids_limit: Some(true),
        available_images,
        disk_available_bytes: None,
    }
}

fn lock(enabled: bool) -> ImageLock {
    ImageLock {
        schema_version: 1,
        image_set_version: "synthetic".into(),
        profiles: BTreeMap::from([(
            "browser_qa".into(),
            vec![ImageVariant {
                platform: "linux/amd64".into(),
                reference: "ghcr.io/example/browser@sha256:synthetic".into(),
                digest: "sha256:abcdef0123456789".into(),
                tool_versions: BTreeMap::new(),
                runtime_enabled: enabled,
                build_recipe_sha256: Some("sha256:synthetic".into()),
            }],
        )]),
    }
}

#[test]
fn image_diagnostic_separates_reviewed_policy_from_local_presence() {
    let image_lock = lock(false);
    let variant = &image_lock.profiles["browser_qa"][0];
    let present = docker_ready(vec![variant.reference.clone()]);
    let missing = docker_ready(Vec::new());

    let text = profile_status(Some(&image_lock), "browser_qa", &present);
    assert!(text.contains("disabled by reviewed image policy"));
    assert!(text.contains("verified locally"));
    assert!(text.contains("abcdef012345"));
    assert!(
        profile_status(Some(&image_lock), "browser_qa", &missing).contains("not verified locally")
    );
}

#[test]
fn reviewer_diagnostic_uses_the_fresh_qa_route_capability() {
    let id = "synthetic";
    let now = crate::persistence::harness_settings::current_unix_ms();
    let snapshot = HarnessCapabilitySnapshotV1 {
        schema_version: 1,
        provider_id: id.into(),
        probe_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        probe_cancellation_generation: 1,
        probe_sequence: 1,
        cli_binary: ReadinessState::Ready,
        cli_version: Some("1.0".into()),
        authentication: ReadinessState::Ready,
        model_catalog: ReadinessState::Ready,
        scriptable_invocation: ReadinessState::Ready,
        docker_mcp_bridge: ReadinessState::Ready,
        execution_target: ReadinessState::Ready,
        failure: None,
        modes: crate::harness::ExecutionMode::ALL
            .iter()
            .map(|mode| crate::execution::contract::ModeReadinessV1 {
                mode: *mode,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
        roles: HarnessRole::ALL
            .iter()
            .map(|role| crate::execution::contract::RoleReadinessV1 {
                role: *role,
                available: ReadinessState::Ready,
                failure: None,
            })
            .collect(),
    };
    let settings = HarnessSettings {
        work_routes: BTreeMap::from([(
            QA.into(),
            WorkRoute {
                harness: id.into(),
                model: None,
            },
        )]),
        discovered: BTreeMap::from([(
            id.into(),
            DetectedHarness {
                status: "ready".into(),
                version: Some("1.0".into()),
                executable: Some("/synthetic/cli".into()),
                diagnostic: None,
                ready: true,
                models: Vec::new(),
                default_model: None,
                model_catalog_fresh: true,
                configuration_required: false,
                implementation_available: true,
                capability_snapshot: Some(snapshot),
                checked_at_unix_ms: now,
            },
        )]),
        ..Default::default()
    };
    assert!(reviewer_status(&settings).contains("ready"));
    assert!(reviewer_status(&HarnessSettings::default()).contains("select a QA-capable CLI"));
}

#[test]
fn preview_relay_is_never_claimed_ready_without_docker() {
    let status = DockerEnvironmentStatus {
        engine_ready: false,
        state: DockerEnvironmentState::NotRunning,
        message: "stopped".into(),
        desktop_vm: false,
        daemon_version: None,
        architecture: None,
        rootless: None,
        cpu_limits: None,
        memory_limits: None,
        pids_limit: None,
        available_images: Vec::new(),
        disk_available_bytes: None,
    };
    assert!(relay_status(&status).contains("unavailable"));
}
