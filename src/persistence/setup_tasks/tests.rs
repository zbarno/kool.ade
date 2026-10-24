use super::*;
use crate::app::setup_attention::catalog::{Finding, FindingStatus, HostKind};

fn snapshot(ids: &[(&str, FindingStatus)], domains: &[&str], at: u64) -> MachineSnapshot {
    MachineSnapshot {
        schema_version: SNAPSHOT_VERSION,
        host: HostKind::Linux,
        checked_at_unix_ms: at,
        findings: ids
            .iter()
            .map(|(id, status)| Finding {
                id: (*id).into(),
                title: (*id).into(),
                explanation: "safe observation".into(),
                next_step: "retry".into(),
                status: *status,
                prerequisites: Vec::new(),
            })
            .collect(),
        checked_domains: domains.iter().map(|s| (*s).into()).collect(),
    }
}

#[test]
fn two_faults_are_distinct_and_repeated_scan_deduplicates() {
    let s = snapshot(
        &[
            ("setup:cli:codex:auth", FindingStatus::NeedsAttention),
            ("setup:docker:rootless", FindingStatus::NeedsAttention),
        ],
        &["cli", "docker"],
        10,
    );
    let mut store = Store::default();
    assert!(store.reconcile(&s).unwrap());
    assert_eq!(store.tasks.len(), 2);
    assert!(!store.reconcile(&s).unwrap());
    assert_eq!(store.tasks["setup:docker:rootless"].events.len(), 1);
}

#[test]
fn unverified_docker_scan_does_not_mark_old_problem_fixed() {
    let mut store = Store::default();
    store
        .reconcile(&snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
            &["docker"],
            1,
        ))
        .unwrap();
    store.reconcile(&snapshot(&[], &["cli"], 2)).unwrap();
    assert_eq!(
        store.tasks["setup:docker:rootless"].state,
        State::NeedsAttention
    );
    store.reconcile(&snapshot(&[], &["docker"], 3)).unwrap();
    assert_eq!(store.tasks["setup:docker:rootless"].state, State::Verified);
    store
        .reconcile(&snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
            &["docker"],
            4,
        ))
        .unwrap();
    assert_eq!(
        store.tasks["setup:docker:rootless"].state,
        State::NeedsAttention
    );
}

#[test]
fn a_changed_docker_failure_is_not_a_verified_repair() {
    let mut store = Store::default();
    store
        .reconcile(&snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
            &[],
            1,
        ))
        .unwrap();
    // A newly stopped engine is a different problem, not proof rootless
    // setup succeeded. Only a healthy rootless engine resolves it.
    store
        .reconcile(&snapshot(
            &[("setup:docker:start", FindingStatus::NeedsAttention)],
            &[],
            2,
        ))
        .unwrap();
    assert_eq!(
        store.tasks["setup:docker:rootless"].state,
        State::NeedsAttention
    );
}

#[test]
fn a_missing_cli_after_auth_failure_is_not_verified_auth() {
    let mut store = Store::default();
    store
        .reconcile(&snapshot(
            &[("setup:cli:codex:auth", FindingStatus::NeedsAttention)],
            &[],
            1,
        ))
        .unwrap();
    store
        .reconcile(&snapshot(
            &[("setup:cli:install", FindingStatus::NeedsAttention)],
            &["cli:scan"],
            2,
        ))
        .unwrap();
    assert_eq!(
        store.tasks["setup:cli:codex:auth"].state,
        State::NeedsAttention
    );
}

#[test]
fn stale_snapshot_cannot_reopen_or_verify_newer_readiness() {
    let mut store = Store::default();
    store
        .reconcile(&snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
            &[],
            5,
        ))
        .unwrap();
    assert!(store.reconcile(&snapshot(&[], &["docker"], 4)).is_err());
    assert_eq!(
        store.tasks["setup:docker:rootless"].state,
        State::NeedsAttention
    );
}

#[test]
fn persists_and_recovers_independently_of_any_project() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("state").join("tasks.json");
    let mut store = Store::default();
    store
        .reconcile(&snapshot(
            &[("setup:cli:install", FindingStatus::NeedsAttention)],
            &["cli"],
            4,
        ))
        .unwrap();
    save_to(&path, &store).unwrap();
    assert_eq!(load_from(&path).unwrap(), store);
}

#[test]
fn refuses_invalid_ids_without_modifying_store() {
    let mut store = Store::default();
    assert!(
        store
            .reconcile(&snapshot(
                &[("setup:../../secret", FindingStatus::NeedsAttention)],
                &["cli"],
                1
            ))
            .is_err()
    );
    assert!(store.tasks.is_empty());
}

#[test]
fn image_evidence_verifies_only_the_exact_profile_and_architecture() {
    let mut store = Store::default();
    store
        .reconcile(&snapshot(
            &[
                ("setup:images:general:amd64", FindingStatus::NeedsAttention),
                ("setup:images:rust:amd64", FindingStatus::NeedsAttention),
            ],
            &[],
            1,
        ))
        .unwrap();
    store
        .reconcile(&snapshot(&[], &["images:rust:amd64"], 2))
        .unwrap();
    assert_eq!(
        store.tasks["setup:images:rust:amd64"].state,
        State::Verified
    );
    assert_eq!(
        store.tasks["setup:images:general:amd64"].state,
        State::NeedsAttention
    );
}
