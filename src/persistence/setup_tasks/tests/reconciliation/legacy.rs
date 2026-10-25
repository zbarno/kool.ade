use crate::app::setup_attention::catalog::FindingStatus;
use crate::persistence::setup_tasks::{
    SetupTaskEventV1, SetupTaskStateV1, SetupTaskStoreV1, SetupTaskV1,
};

use super::super::support::{scan, snapshot};

#[test]
fn legacy_docker_task_is_superseded_by_a_specific_failure_without_generic_engine_evidence() {
    let mut store = SetupTaskStoreV1::default();
    store.tasks.insert(
        "setup:legacy:docker".into(),
        SetupTaskV1 {
            schema_version: 1,
            id: "setup:legacy:docker".into(),
            title: "Legacy Docker setup".into(),
            explanation: "Migrated generic Docker notice.".into(),
            next_step: "Recheck Docker.".into(),
            dependencies: Vec::new(),
            blocked_by: Vec::new(),
            state: SetupTaskStateV1::NeedsVerification,
            created_at_unix_ms: 0,
            observed_at_unix_ms: 0,
            scan_generation: 0,
            verified_at_unix_ms: None,
            verified_result: None,
            attempts: Vec::new(),
            events: vec![SetupTaskEventV1 {
                observed_at_unix_ms: 0,
                scan_generation: 0,
                state: SetupTaskStateV1::NeedsVerification,
            }],
            legacy_source_id: Some("setup:docker".into()),
            superseded_by: Vec::new(),
        },
    );

    scan(
        &mut store,
        &snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention, &[])],
            &["docker:rootless"],
            1,
        ),
    )
    .unwrap();

    let legacy = &store.tasks["setup:legacy:docker"];
    assert_eq!(legacy.state, SetupTaskStateV1::Superseded);
    assert_eq!(legacy.superseded_by, ["setup:docker:rootless"]);
}
