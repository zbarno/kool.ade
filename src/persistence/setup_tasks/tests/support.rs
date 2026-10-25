use crate::app::setup_attention::catalog::{
    Finding, FindingStatus, HostKind, MachineSnapshot, SCHEMA_VERSION,
};

use super::super::{SetupTaskStateV1, SetupTaskStoreV1};

pub fn snapshot(
    findings: &[(&str, FindingStatus, &[&str])],
    domains: &[&str],
    at: u64,
) -> MachineSnapshot {
    MachineSnapshot {
        schema_version: SCHEMA_VERSION,
        host: HostKind::Linux,
        checked_at_unix_ms: at,
        findings: findings
            .iter()
            .map(|(id, status, prerequisites)| Finding {
                id: (*id).into(),
                title: (*id).into(),
                explanation: "synthetic readiness finding".into(),
                next_step: "retry the readiness scan".into(),
                status: *status,
                prerequisites: prerequisites.iter().map(|value| (*value).into()).collect(),
            })
            .collect(),
        checked_domains: domains.iter().map(|domain| (*domain).into()).collect(),
    }
}

pub fn scan(store: &mut SetupTaskStoreV1, snapshot: &MachineSnapshot) -> anyhow::Result<bool> {
    store.latest_scan_generation += 1;
    store.reconcile(snapshot, store.latest_scan_generation)
}

pub fn ready_store(id: &str) -> SetupTaskStoreV1 {
    let mut store = SetupTaskStoreV1::default();
    scan(
        &mut store,
        &snapshot(
            &[(id, FindingStatus::NeedsAttention, &[])],
            &["docker:rootless"],
            1,
        ),
    )
    .unwrap();
    assert_eq!(store.tasks[id].state, SetupTaskStateV1::ReadyToFix);
    store
}
