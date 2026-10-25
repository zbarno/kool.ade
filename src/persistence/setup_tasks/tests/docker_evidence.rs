use crate::app::setup_attention::catalog::{FindingStatus, HostKind};

use super::super::{SetupTaskStateV1, SetupTaskStoreV1};
use super::support::{scan, snapshot};

#[test]
fn desktop_vm_recovery_verifies_engine_readiness_but_not_transport_certification() {
    let mut store = SetupTaskStoreV1::default();
    let mut unavailable = snapshot(
        &[
            ("setup:docker:desktop", FindingStatus::NeedsAttention, &[]),
            (
                "setup:docker:desktop-transport",
                FindingStatus::Unsupported,
                &[],
            ),
        ],
        &[],
        1,
    );
    unavailable.host = HostKind::MacOs;
    scan(&mut store, &unavailable).unwrap();

    let mut engine_ready = snapshot(
        &[(
            "setup:docker:desktop-transport",
            FindingStatus::Unsupported,
            &[],
        )],
        &["docker"],
        2,
    );
    engine_ready.host = HostKind::MacOs;
    scan(&mut store, &engine_ready).unwrap();

    let desktop = &store.tasks["setup:docker:desktop"];
    assert_eq!(desktop.state, SetupTaskStateV1::ReadyToFix);
    assert!(desktop.verified_result.is_none());

    let mut desktop_ready = snapshot(&[], &["docker:desktop"], 3);
    desktop_ready.host = HostKind::MacOs;
    scan(&mut store, &desktop_ready).unwrap();

    let desktop = &store.tasks["setup:docker:desktop"];
    assert_eq!(desktop.state, SetupTaskStateV1::Verified);
    assert_eq!(
        desktop.verified_result.as_ref().unwrap().evidence_domain,
        "docker:desktop"
    );
    let transport = &store.tasks["setup:docker:desktop-transport"];
    assert_eq!(transport.state, SetupTaskStateV1::Unsupported);
    assert!(transport.verified_result.is_none());
}

#[test]
fn desktop_probe_resolves_generic_engine_findings_with_desktop_specific_evidence() {
    for id in [
        "setup:docker:install",
        "setup:docker:start",
        "setup:docker:permission",
        "setup:docker:windows-containers",
    ] {
        let mut store = SetupTaskStoreV1::default();
        let mut unavailable = snapshot(&[(id, FindingStatus::NeedsAttention, &[])], &[], 1);
        unavailable.host = HostKind::MacOs;
        scan(&mut store, &unavailable).unwrap();

        let mut desktop_ready = snapshot(&[], &["docker:desktop"], 2);
        desktop_ready.host = HostKind::MacOs;
        scan(&mut store, &desktop_ready).unwrap();

        let task = &store.tasks[id];
        assert_eq!(task.state, SetupTaskStateV1::Verified, "{id}");
        assert_eq!(
            task.verified_result.as_ref().unwrap().evidence_domain,
            "docker:desktop",
            "{id}"
        );
        store.validate().unwrap();
    }
}

#[test]
fn desktop_evidence_does_not_verify_linux_only_rootless_readiness() {
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention, &[])],
            &[],
            1,
        ),
    )
    .unwrap();

    let mut desktop_ready = snapshot(&[], &["docker:desktop"], 2);
    desktop_ready.host = HostKind::MacOs;
    scan(&mut store, &desktop_ready).unwrap();

    let rootless = &store.tasks["setup:docker:rootless"];
    assert_eq!(rootless.state, SetupTaskStateV1::ReadyToFix);
    assert!(rootless.verified_result.is_none());
}
