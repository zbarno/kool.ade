use crate::{
    execution::{
        contract::ReadinessState,
        docker::environment::{DockerEnvironmentState, DockerEnvironmentStatus},
    },
    harness::discovery::{ExecutionCapability, ModelCatalogStatus, ProbeResult, ProbeStatus},
};

use super::super::{
    EvidenceState, HostKind, PrerequisiteCapability, PrerequisiteDomain, PrerequisiteSubject,
    ProviderIdV1,
};

mod desktop;
mod docker_failures;
mod generation;
mod git;

fn docker(state: DockerEnvironmentState, engine_ready: bool) -> DockerEnvironmentStatus {
    DockerEnvironmentStatus {
        state,
        engine_ready,
        message: "private /home/secret account detail must not be persisted".into(),
        desktop_vm: false,
        daemon_version: None,
        architecture: Some("amd64".into()),
        rootless: Some(false),
        cpu_limits: None,
        memory_limits: None,
        pids_limit: None,
        cgroups_supported: engine_ready.then_some(true),
        available_images: Vec::new(),
        disk_available_bytes: None,
        endpoint_fingerprint: None,
        daemon_identity_fingerprint: None,
    }
}

fn provider(id: &str) -> ProbeResult {
    ProbeResult {
        provider_id: id.into(),
        run_generation: 11,
        started_at_unix_ms: 100,
        binary_path: Some("/home/private-account/.local/bin/codex".into()),
        version: Some("1.2.3".into()),
        authentication: ReadinessState::Ready,
        model_catalog_status: ModelCatalogStatus::Fresh,
        models: vec!["secret-model-label".into()],
        execution_capability: ExecutionCapability {
            safe_for_planning: ReadinessState::Ready,
            ..ExecutionCapability::default()
        },
        status: ProbeStatus::Available,
        diagnostic_message: Some("API_TOKEN=private-value".into()),
        ..ProbeResult::default()
    }
}

#[test]
fn typed_evidence_redacts_provider_paths_models_and_diagnostics() {
    let scan = super::super::assess_for_scan(
        &[provider("codex")],
        Some("codex"),
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        200,
        11,
    );
    let encoded = serde_json::to_string(&scan).unwrap();
    for private in [
        "/home/private-account",
        "secret-model-label",
        "API_TOKEN",
        "private-value",
        "private /home/secret",
    ] {
        assert!(!encoded.contains(private));
    }
    assert!(scan.validate().is_ok());
}

#[test]
fn provider_evidence_is_typed_for_all_supported_provider_ids() {
    let mut probes = [
        "pi",
        "codex",
        "claude",
        "opencode",
        "copilot",
        "antigravity",
    ]
    .into_iter()
    .map(provider)
    .collect::<Vec<_>>();
    probes[1].binary_path = None;
    probes[1].status = ProbeStatus::Missing;
    probes[2].authentication = ReadinessState::Unavailable;
    probes[3].authentication = ReadinessState::Unknown;
    probes[4].model_catalog_status = ModelCatalogStatus::Stale;
    probes[5].status = ProbeStatus::Failed;
    probes[5].binary_path = None;
    let scan = super::super::assess_for_scan(
        &probes,
        Some("codex"),
        &docker(DockerEnvironmentState::NotInstalled, false),
        HostKind::Linux,
        200,
        11,
    );
    let state = |provider, capability| {
        scan.probe_evidence
            .iter()
            .find(|evidence| {
                evidence.prerequisite.domain == PrerequisiteDomain::Provider
                    && evidence.prerequisite.capability == capability
                    && evidence.prerequisite.subject == PrerequisiteSubject::Provider(provider)
            })
            .map(|evidence| evidence.state)
            .unwrap()
    };
    assert_eq!(
        state(
            ProviderIdV1::Pi,
            PrerequisiteCapability::ProviderAuthentication
        ),
        EvidenceState::Ready
    );
    assert_eq!(
        state(
            ProviderIdV1::Codex,
            PrerequisiteCapability::ProviderExecutable
        ),
        EvidenceState::Missing
    );
    assert_eq!(
        state(
            ProviderIdV1::Claude,
            PrerequisiteCapability::ProviderAuthentication
        ),
        EvidenceState::Missing
    );
    assert_eq!(
        state(
            ProviderIdV1::OpenCode,
            PrerequisiteCapability::ProviderAuthentication
        ),
        EvidenceState::Unknown
    );
    assert_eq!(
        state(
            ProviderIdV1::Copilot,
            PrerequisiteCapability::ProviderModels
        ),
        EvidenceState::Stale
    );
    assert_eq!(
        state(
            ProviderIdV1::Antigravity,
            PrerequisiteCapability::ProviderExecutable
        ),
        EvidenceState::Failed
    );
}

#[test]
fn mac_and_windows_report_unknown_desktop_and_wsl_facts_without_claiming_support() {
    let mac = super::super::assess_for_scan(
        &[],
        None,
        &docker(DockerEnvironmentState::DesktopVmRequired, false),
        HostKind::MacOs,
        200,
        11,
    );
    assert_eq!(
        mac.capability_matrix.native_controller,
        super::super::SupportClaim::Planned
    );
    assert!(mac.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.capability == PrerequisiteCapability::DockerDesktopVm
            && evidence.state == EvidenceState::Missing
    }));

    let windows = super::super::assess_for_scan(
        &[],
        None,
        &docker(DockerEnvironmentState::UntrustedEndpoint, false),
        HostKind::Windows,
        201,
        12,
    );
    assert_eq!(
        windows.capability_matrix.native_controller,
        super::super::SupportClaim::Planned
    );
    assert_eq!(
        windows.capability_matrix.windows_wsl2,
        EvidenceState::Unknown
    );
    assert!(windows.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.capability == PrerequisiteCapability::DockerEndpoint
            && evidence.state == EvidenceState::Failed
    }));
    assert!(windows.probe_evidence.iter().any(|evidence| {
        evidence.prerequisite.capability == PrerequisiteCapability::DockerDesktopTrust
            && evidence.state == EvidenceState::Unknown
    }));
}
