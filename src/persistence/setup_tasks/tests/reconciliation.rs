use super::*;

mod images;
mod migrations;

#[test]
fn two_faults_are_distinct_and_repeated_scan_deduplicates() {
    let s = snapshot(
        &[
            ("setup:cli:codex:auth", FindingStatus::NeedsAttention),
            ("setup:docker:rootless", FindingStatus::NeedsAttention),
        ],
        &[],
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
            &[],
            1,
        ))
        .unwrap();
    store.reconcile(&snapshot(&[], &[], 2)).unwrap();
    assert_eq!(
        store.tasks["setup:docker:rootless"].state,
        State::NeedsAttention
    );
    store.reconcile(&snapshot(&[], &["docker"], 3)).unwrap();
    assert_eq!(store.tasks["setup:docker:rootless"].state, State::Verified);
    store
        .reconcile(&snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
            &[],
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
            &[],
            2,
        ))
        .unwrap();
    assert_eq!(
        store.tasks["setup:cli:codex:auth"].state,
        State::NeedsAttention
    );
}

#[test]
fn git_task_requires_a_positive_git_probe_before_resolution() {
    let mut store = Store::default();
    store
        .reconcile(&snapshot(
            &[("setup:git:install", FindingStatus::NeedsAttention)],
            &[],
            1,
        ))
        .unwrap();
    store.reconcile(&snapshot(&[], &[], 2)).unwrap();
    assert_eq!(
        store.tasks["setup:git:install"].state,
        State::NeedsAttention
    );
    store
        .reconcile(&snapshot(&[], &["git:executable"], 3))
        .unwrap();
    assert_eq!(store.tasks["setup:git:install"].state, State::Verified);
}

#[test]
fn healthy_engine_resolves_legacy_os_architecture_and_workspace_findings() {
    for id in [
        "setup:docker:os",
        "setup:docker:architecture",
        "setup:docker:workspace",
    ] {
        let mut store = Store::default();
        store
            .reconcile(&snapshot(&[(id, FindingStatus::NeedsAttention)], &[], 1))
            .unwrap();
        store.reconcile(&snapshot(&[], &["docker"], 2)).unwrap();
        assert_eq!(store.tasks[id].state, State::Verified, "{id}");
    }
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
fn equal_timestamp_scan_cannot_replace_a_different_observation() {
    let mut store = Store::default();
    store
        .reconcile(&snapshot(
            &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
            &[],
            20,
        ))
        .unwrap();

    let conflicting = snapshot(
        &[("setup:docker:start", FindingStatus::NeedsAttention)],
        &[],
        20,
    );
    assert!(store.reconcile(&conflicting).is_err());
    assert!(store.tasks.contains_key("setup:docker:rootless"));
    assert!(!store.tasks.contains_key("setup:docker:start"));
}

#[test]
fn ordering_uses_scan_start_when_slow_scans_finish_out_of_order() {
    let mut store = Store::default();
    let mut slow = snapshot(
        &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
        &[],
        30,
    );
    slow.scan_started_at_unix_ms = 10;
    store.reconcile(&slow).unwrap();

    let mut fast = snapshot(
        &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
        &[],
        25,
    );
    fast.scan_started_at_unix_ms = 20;
    fast.findings[0].title = "newer fast scan".into();
    store.reconcile(&fast).unwrap();

    assert_eq!(store.last_scan_started_at_unix_ms, 20);
    assert_eq!(store.last_observed_at_unix_ms, 30);
    assert_eq!(
        store.tasks["setup:docker:rootless"].title,
        "newer fast scan"
    );
}

#[test]
fn older_scan_start_is_rejected_even_when_it_finishes_later() {
    let mut store = Store::default();
    let mut newer = snapshot(
        &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
        &[],
        25,
    );
    newer.scan_started_at_unix_ms = 20;
    newer.findings[0].title = "newer scan".into();
    store.reconcile(&newer).unwrap();

    let mut older = snapshot(
        &[("setup:docker:rootless", FindingStatus::NeedsAttention)],
        &[],
        30,
    );
    older.scan_started_at_unix_ms = 10;
    assert!(store.reconcile(&older).is_err());
    assert_eq!(store.tasks["setup:docker:rootless"].title, "newer scan");
}

#[test]
fn empty_scan_persists_a_global_freshness_watermark() {
    let mut store = Store::default();
    assert!(store.reconcile(&snapshot(&[], &[], 20)).unwrap());
    assert_eq!(store.last_observed_at_unix_ms, 20);
    assert!(store.reconcile(&snapshot(&[], &[], 19)).is_err());
}

#[test]
fn persists_and_recovers_independently_of_any_project() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("state").join("tasks.json");
    let mut store = Store::default();
    store
        .reconcile(&snapshot(
            &[("setup:cli:install", FindingStatus::NeedsAttention)],
            &[],
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
                &[],
                1
            ))
            .is_err()
    );
    assert!(store.tasks.is_empty());
}
