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
        &["docker:engine"],
        2,
    );
    engine_ready.host = HostKind::MacOs;
    scan(&mut store, &engine_ready).unwrap();

    let desktop = &store.tasks["setup:docker:desktop"];
    assert_eq!(desktop.state, SetupTaskStateV1::Verified);
    assert_eq!(
        desktop.verified_result.as_ref().unwrap().evidence_domain,
        "docker:engine"
    );
    let transport = &store.tasks["setup:docker:desktop-transport"];
    assert_eq!(transport.state, SetupTaskStateV1::Unsupported);
    assert!(transport.verified_result.is_none());
}
