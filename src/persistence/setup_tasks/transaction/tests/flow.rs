//! Persisted flow-state boundary tests: insert gating, change-gated updates,
//! in-place v2->v3 migration, record limits, and validation of stored
//! content. Temp directory and local filesystem only; no external services,
//! low setup cost.

use super::super::flow::{insert_flow_at, replace_terminal_flow_at, update_flow_at};
use crate::persistence::setup_tasks::{FlowPhaseV1, SetupFlowStateV1, SetupTaskStoreV1};
use std::collections::{BTreeMap, BTreeSet};

fn seed_store(path: &std::path::Path, schema_version: u8, flow: Option<SetupFlowStateV1>) {
    let store = SetupTaskStoreV1 {
        schema_version,
        latest_scan_generation: 1,
        tasks: BTreeMap::new(),
        required_image_profiles: BTreeSet::new(),
        setup_flow: flow,
    };
    std::fs::write(path, serde_json::to_vec(&store).unwrap()).unwrap();
}

fn fresh_flow() -> SetupFlowStateV1 {
    SetupFlowStateV1::start_new("flow-unit-01".to_string(), 100).unwrap()
}

#[test]
fn a_second_insert_into_an_occupied_slot_is_a_no_op() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.json");
    seed_store(&path, 3, None);

    assert!(insert_flow_at(&path, fresh_flow()).unwrap());
    let challenger = SetupFlowStateV1::start_new("flow-other-02".to_string(), 200).unwrap();
    assert!(!insert_flow_at(&path, challenger).unwrap());

    let incumbent = update_flow_at(&path, |_| false).unwrap().unwrap();
    assert_eq!(incumbent.flow_id, "flow-unit-01");
}

#[test]
fn absent_flow_reads_back_none_without_invoking_the_update() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.json");
    seed_store(&path, 3, None);

    let mut update_calls = 0;
    let read = update_flow_at(&path, |state| {
        update_calls += 1;
        state.phase = FlowPhaseV1::Running;
        true
    })
    .unwrap();
    assert!(read.is_none());
    assert_eq!(update_calls, 0);
}

#[test]
fn schema_v2_store_migrates_to_v3_when_the_flow_first_moves() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.json");
    seed_store(&path, 2, None);

    assert!(insert_flow_at(&path, fresh_flow()).unwrap());

    let on_disk: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(on_disk["schemaVersion"], 3);
    assert_eq!(on_disk["setupFlow"]["schema"], 1);

    let phase_read = update_flow_at(&path, |state| {
        assert!(matches!(state.phase, FlowPhaseV1::Running));
        state.phase = FlowPhaseV1::Paused;
        true
    })
    .unwrap();
    assert!(matches!(phase_read.unwrap().phase, FlowPhaseV1::Paused));
}

#[test]
fn stored_record_limits_survive_round_trip_through_the_store() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.json");
    seed_store(&path, 3, None);

    assert!(insert_flow_at(&path, fresh_flow()).unwrap());
    let grown = update_flow_at(&path, |state| {
        for i in 0..40u32 {
            state.last_completed.push(format!("setup:r:{i:02}"));
            state.skipped.push(format!("setup:r:{i:02}"));
        }
        true
    })
    .unwrap()
    .unwrap();
    assert_eq!(grown.last_completed.len(), 16);
    assert_eq!(grown.skipped.len(), 16);

    // A second reader sees the trimmed, deduped lists.
    let reread = update_flow_at(&path, |_| false).unwrap().unwrap();
    assert_eq!(reread.last_completed.len(), 16);
    assert_eq!(reread.skipped.len(), 16);
}

#[test]
fn invalid_stored_flow_is_rejected_instead_of_loaded() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.json");
    seed_store(&path, 3, None);
    let bad = SetupFlowStateV1 {
        schema: 9,
        ..SetupFlowStateV1::start_new("flow-unit-01".to_string(), 100).unwrap()
    };
    seed_store(&path, 3, Some(bad));

    let err = update_flow_at(&path, |_| true).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("flow"), "{err}");
}

#[test]
fn a_live_incumbent_is_kept_even_when_restart_requests_replacement() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.json");
    // Incumbent is mid-run: the replace terminal primitive must respect it.
    seed_store(&path, 3, Some(fresh_flow()));

    let challenger = SetupFlowStateV1::start_new("flow-challenger-1".to_string(), 200).unwrap();
    assert!(!replace_terminal_flow_at(&path, challenger).unwrap());

    let incumbent = update_flow_at(&path, |_| false).unwrap().unwrap();
    assert_eq!(incumbent.flow_id, "flow-unit-01");
    assert!(matches!(incumbent.phase, FlowPhaseV1::Running));
}

#[test]
fn a_finished_incumbent_yields_to_an_explicit_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tasks.json");
    let mut done = fresh_flow();
    done.advance(200, |state| {
        state.phase =
            FlowPhaseV1::Complete(crate::persistence::setup_tasks::DoneOutcomeV1::FullyPrepared);
        true
    });
    seed_store(&path, 3, Some(done));

    let challenger = SetupFlowStateV1::start_new("flow-fresh-02".to_string(), 300).unwrap();
    assert!(replace_terminal_flow_at(&path, challenger).unwrap());

    let current = update_flow_at(&path, |_| false).unwrap().unwrap();
    assert_eq!(current.flow_id, "flow-fresh-02");
}
