//! Unit tests for the accrual rules and the process-global host's ledger
//! integration. Pure-engine tests use synthetic clocks; host tests are
//! serialized (shared global) and exercise real ledger files.

use super::host::{
    activate_project, app_close_flush, drain_errors, reset_for_tests, span_begin, span_for_ticket,
};
use super::{Accruer, AgentSpan, MIN_ACCRUED_SECONDS, StartOutcome, StopReason};
use crate::artifacts::time_ledger::{self, EndStatus, IntervalRow};
use crate::core::project_repos::{ProjectManifest, Repository};
use std::path::{Path, PathBuf};

// ------------------------------- pure engine -------------------------------

fn span(ws: &str, item: &str) -> AgentSpan {
    AgentSpan {
        repo_root: PathBuf::from("/unused-in-pure-tests"),
        repo_id: "root".into(),
        workspace_id: ws.into(),
        item_uid: item.into(),
    }
}

#[test]
fn active_process_lifetime_produces_exactly_one_closed_interval() {
    let mut acc = Accruer::new();
    assert_eq!(
        acc.on_active_start(&span("alpha", "t-1"), "sess-1", 1000),
        StartOutcome::Opened
    );
    assert!(acc.is_open("alpha"));
    let row = acc
        .on_active_stop("alpha", StopReason::Completed, 1061)
        .unwrap();
    assert_eq!(
        (row.start_epoch_s, row.end_epoch_s, row.reason),
        (1000, 1061, StopReason::Completed)
    );
    assert_eq!(row.end_status, EndStatus::Ended);
    assert_eq!(row.session_id, "sess-1");
    assert_eq!(acc.open_count(), 0);
    assert!(
        acc.on_active_stop("alpha", StopReason::Idle, 1100)
            .is_none()
    );
}

#[test]
fn subsecond_spans_are_dropped_and_one_second_is_the_floor() {
    let mut acc = Accruer::new();
    acc.on_active_start(&span("alpha", "t"), "s", 500);
    assert!(acc.on_active_stop("alpha", StopReason::Idle, 500).is_none());
    acc.on_active_start(&span("alpha", "t"), "s", 501);
    assert!(acc.on_active_stop("alpha", StopReason::Idle, 501).is_none());
    acc.on_active_start(&span("alpha", "t"), "s", 502);
    let row = acc
        .on_active_stop("alpha", StopReason::Idle, 502 + MIN_ACCRUED_SECONDS)
        .unwrap();
    assert_eq!(row.duration_secs(), 1);
    assert_eq!(acc.stats(), (1, 0, 2));
}

#[test]
fn backwards_clock_never_creates_a_row() {
    let mut acc = Accruer::new();
    acc.on_active_start(&span("alpha", "t"), "s", 9000);
    assert!(
        acc.on_active_stop("alpha", StopReason::Paused, 8999)
            .is_none()
    );
}

#[test]
fn overlapping_runs_share_one_slot_regardless_of_who_stops_first() {
    // Direction A: the original owner stops first; the suppressed peer's
    // later stop must not mint a second row for the same workspace.
    let mut acc = Accruer::new();
    acc.on_active_start(&span("beta", "t-a"), "sess-A", 100);
    assert_eq!(
        acc.on_active_start(&span("beta", "t-b"), "sess-B", 104),
        StartOutcome::OverlapSuppressed
    );
    assert_eq!(acc.open_count(), 1);
    let first = acc
        .on_active_stop("beta", StopReason::Completed, 130)
        .unwrap();
    let second = acc.on_active_stop("beta", StopReason::Completed, 140);
    assert!(second.is_none());
    assert_eq!(first.duration_secs(), 30);
    assert_eq!(
        first.session_id, "sess-A",
        "the owning slot survives the overlap"
    );

    // Direction B: the suppressed peer observes a stop first; the settlement
    // still credits the ORIGINAL start instant, and the owner's own stop
    // afterwards yields nothing (no double count).
    let mut acc = Accruer::new();
    acc.on_active_start(&span("beta", "t-a"), "sess-A", 200);
    assert_eq!(
        acc.on_active_start(&span("beta", "t-b"), "sess-B", 203),
        StartOutcome::OverlapSuppressed
    );
    let peer_seen = acc
        .on_active_stop("beta", StopReason::Completed, 220)
        .unwrap();
    let owner_late = acc.on_active_stop("beta", StopReason::Completed, 250);
    assert!(owner_late.is_none());
    assert_eq!(peer_seen.start_epoch_s, 200);
    assert_eq!(peer_seen.session_id, "sess-A");
    assert!(
        peer_seen.duration_secs() <= 250 - 200,
        "union: never above wall rate"
    );
}

#[test]
fn distinct_workspaces_accrue_independently() {
    let mut acc = Accruer::new();
    acc.on_active_start(&span("w1", "a"), "s1", 10);
    acc.on_active_start(&span("w2", "b"), "s2", 12);
    assert_eq!(acc.open_count(), 2);
    let r1 = acc.on_active_stop("w1", StopReason::Completed, 40).unwrap();
    let r2 = acc.on_active_stop("w2", StopReason::Idle, 50).unwrap();
    assert_eq!((r1.duration_secs(), r2.duration_secs()), (30, 38));
}

#[test]
fn operator_wait_freezes_accrual_and_release_dates_the_fresh_interval() {
    let mut acc = Accruer::new();
    acc.on_active_start(&span("gamma", "t"), "s", 0);
    let held = acc
        .on_active_stop("gamma", StopReason::OperatorWait, 180)
        .unwrap();
    // Three further minutes pass with no agent process; release must not
    // retroactively credit them (backfill-zero).
    acc.on_active_start(&span("gamma", "t"), "s", 360);
    let resumed = acc
        .on_active_stop("gamma", StopReason::Completed, 420)
        .unwrap();
    let total: u64 = [&held, &resumed].iter().map(|r| r.duration_secs()).sum();
    assert_eq!((held.end_epoch_s, resumed.start_epoch_s), (180, 360));
    assert_eq!(total, 240, "the 180-second walk-away must stay unbilled");
}

#[test]
fn app_close_finalizes_every_open_slot_as_interrupted_discard() {
    let mut acc = Accruer::new();
    acc.on_active_start(&span("zeta", "t"), "s", 10);
    acc.on_active_start(&span("mid", "t"), "s", 12);
    let flushed = acc.on_app_close(50);
    assert_eq!(flushed.len(), 2);
    assert!(
        flushed
            .iter()
            .all(|r| r.end_status == EndStatus::InterruptedDiscard)
    );
    assert_eq!(
        flushed
            .iter()
            .map(|r| r.span.workspace_id.as_str())
            .collect::<Vec<_>>(),
        ["mid", "zeta"]
    );
    assert_eq!(acc.open_count(), 0);
    // A restart cannot re-credit the abandoned intervals.
    assert!(
        acc.on_active_stop("zeta", StopReason::Completed, 60)
            .is_none()
    );
}

#[test]
fn app_close_respects_the_noise_floor() {
    let mut acc = Accruer::new();
    acc.on_active_start(&span("delta", "t"), "s", 49);
    assert!(acc.on_app_close(49).is_empty());
}

#[test]
fn rapid_start_stop_churn_stays_silent() {
    let mut acc = Accruer::new();
    for i in 0..10 {
        acc.on_active_start(&span("epsilon", "t"), "s", i * 3);
        assert!(
            acc.on_active_stop("epsilon", StopReason::Idle, i * 3)
                .is_none()
        );
    }
    assert_eq!(acc.stats(), (0, 0, 10));
}

#[test]
fn closed_intervals_map_on_to_valid_ledger_rows() {
    let mut acc = Accruer::new();
    acc.on_active_start(&span("omega", "task-9"), "sess", 7);
    let row: IntervalRow = acc
        .on_active_stop("omega", StopReason::Completed, 27)
        .unwrap()
        .to_row();
    assert_eq!(
        (
            row.repo_id.as_str(),
            row.workspace_id.as_str(),
            row.item_uid.as_str(),
            row.session_id.as_str()
        ),
        ("root", "omega", "task-9", "sess")
    );
    assert_eq!(row.feature_ref, None);
    assert_eq!(row.worker_pid, None);
    assert_eq!(row.end_status, EndStatus::Ended);
}

// ---------------------------------- host ------------------------------------

static GLUE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
static SCRATCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn scratch(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "koolade-accrual-{}-{}-{}",
        tag,
        std::process::id(),
        SCRATCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ))
}

fn activate(root: &Path) {
    reset_for_tests();
    let manifest = ProjectManifest {
        repositories: vec![Repository {
            id: "root".into(),
            role: "Root repository".into(),
            remote: String::new(),
            display_name: None,
        }],
    };
    activate_project(root, &manifest, "unit-session");
}

fn host_span(root: &Path, ws: &str, item: &str) -> AgentSpan {
    AgentSpan {
        repo_root: root.to_path_buf(),
        repo_id: "root".into(),
        workspace_id: ws.into(),
        item_uid: item.into(),
    }
}

fn rows(root: &Path) -> Vec<IntervalRow> {
    time_ledger::load(root).expect("ledger loads")
}

/// Serialized fixture for tests that touch the process-global host.
fn with_global<F: FnOnce(&Path)>(tag: &str, body: F) {
    let _serial = GLUE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    reset_for_tests();
    let root = scratch(tag);
    std::fs::create_dir_all(&root).unwrap();
    body(root.as_path());
    std::fs::remove_dir_all(&root).ok();
    reset_for_tests();
}

#[test]
fn span_lifecycle_appends_a_real_ledger_row() {
    with_global("e2e", |root| {
        activate(root);
        let handle = span_begin(&host_span(root, "root", "task-1")).expect("metering active");
        std::thread::sleep(std::time::Duration::from_millis(1200));
        handle.close(StopReason::Completed);
        let rs = rows(root);
        assert_eq!(rs.len(), 1, "{rs:?}");
        assert_eq!(rs[0].workspace_id, "root");
        assert_eq!(rs[0].item_uid, "task-1");
        assert_eq!(rs[0].session_id, "unit-session");
        assert_eq!(rs[0].end_status, EndStatus::Ended);
        let dur = rs[0].end_epoch_s.expect("closed") - rs[0].start_epoch_s;
        assert!(dur >= MIN_ACCRUED_SECONDS, "noise floor upheld: {dur}s");
        assert!(drain_errors().is_empty());
    });
}

#[test]
fn overlapping_threads_settle_into_a_single_row() {
    with_global("threads", |root| {
        activate(root);
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut joins = Vec::new();
        for _ in 0..2 {
            let root2 = root.to_path_buf();
            let b2 = barrier.clone();
            joins.push(std::thread::spawn(move || {
                let _guard = span_begin(&host_span(&root2, "shared", "task-x"))
                    .or_else(|| span_begin(&host_span(&root2, "shared", "task-y")));
                b2.wait();
                std::thread::sleep(std::time::Duration::from_millis(1300));
            }));
        }
        for handle in joins {
            handle.join().unwrap();
        }
        let rs = rows(root);
        assert_eq!(
            rs.len(),
            1,
            "concurrent same-workspace agents count once: {rs:?}"
        );
        assert_eq!(rs[0].end_status, EndStatus::Ended);
    });
}

#[test]
fn app_close_flush_persists_discards_and_the_restart_bill_is_fresh() {
    with_global("shutdown", |root| {
        activate(root);
        let sp_a = host_span(root, "root", "live-a");
        let _held = span_begin(&sp_a).expect("opens");
        let _held_b = span_begin(&host_span(root, "side", "live-b")).expect("opens");
        // Age the spans past the noise floor so close-minted discards persist.
        std::thread::sleep(std::time::Duration::from_millis(1200));
        app_close_flush();
        let rs = rows(root);
        assert_eq!(rs.len(), 2, "{rs:?}");
        assert!(
            rs.iter()
                .all(|r| r.end_status == EndStatus::InterruptedDiscard)
        );
        // Restart: re-running the same task bills only the NEW active span.
        let reopened = span_begin(&sp_a).expect("fresh process state");
        std::thread::sleep(std::time::Duration::from_millis(1200));
        reopened.close(StopReason::Completed);
        let rs = rows(root);
        assert_eq!(rs.len(), 3, "two discards plus one fresh interval: {rs:?}");
    });
}

#[test]
fn ledger_write_failure_surfaces_and_never_blocks_the_span() {
    with_global("failure", |root| {
        // Occupy the ledger's parent slot with a FILE so every append fails.
        let state_file = root.join(".koolade-packet/state");
        std::fs::create_dir_all(state_file.parent().unwrap()).unwrap();
        std::fs::write(&state_file, b"occupied").unwrap();
        activate(root);
        let handle = span_begin(&host_span(root, "root", "blocked-1")).expect("active");
        std::thread::sleep(std::time::Duration::from_millis(1200));
        handle.close(StopReason::Completed);
        let notes = drain_errors();
        assert_eq!(notes.len(), 1, "one surfaceable failure expected");
        assert!(notes[0].contains("Time ledger"), "{notes:?}");
        assert!(rows(root).is_empty());
        // Recovery: heal the slot; metering keeps working.
        std::fs::remove_file(&state_file).unwrap();
        let healed = span_begin(&host_span(root, "root", "next-1")).expect("active");
        std::thread::sleep(std::time::Duration::from_millis(1200));
        healed.close(StopReason::Completed);
        assert!(drain_errors().is_empty());
        assert_eq!(rows(root).len(), 1);
    });
}

#[test]
fn inactive_metering_is_a_complete_noop() {
    let _serial = GLUE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    reset_for_tests();
    let root = scratch("noop");
    std::fs::create_dir_all(&root).unwrap();
    let ghost = host_span(&root, "root", "ghost");
    assert!(span_begin(&ghost).is_none(), "no project, no metering");
    app_close_flush(); // idempotent with nobody activated
    assert!(drain_errors().is_empty());
    std::fs::remove_dir_all(&root).ok();
    reset_for_tests();
}

// ----------------------------- attribution input ----------------------------

#[test]
fn span_resolution_prefers_stable_identifiers_over_paths() {
    let root = scratch("attr");
    std::fs::create_dir_all(&root).unwrap();
    let sp = span_for_ticket(
        &root,
        "# Ticket\n\nRepository: root\n",
        None,
        Some("0a1b2c3d-1111-4222-8333-444455556666"),
    )
    .expect("fallback manifest resolves the sole repository");
    assert_eq!(sp.repo_root, root);
    assert_eq!(sp.repo_id, "root");
    assert_eq!(sp.workspace_id, "root");
    assert_eq!(sp.item_uid, "0a1b2c3d-1111-4222-8333-444455556666");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn identifier_hygiene_falls_back_instead_of_poisoning_rows() {
    let root = scratch("hygiene");
    std::fs::create_dir_all(&root).unwrap();
    let text = "# T\n\nRepository: sidecar\n";
    let sp = span_for_ticket(&root, text, None, Some("bad:id,name"))
        .expect("legacy marker resolves to the named workspace");
    assert_eq!(sp.workspace_id, "sidecar");
    assert_eq!(sp.item_uid, "task", "colon/comma item uid falls back");
    let stemmed =
        span_for_ticket(&root, "/x/projects/F7-TASK-clean-stem.md", None, None).expect("resolves");
    assert_eq!(
        stemmed.item_uid, "F7-TASK-clean-stem",
        "missing uid derives from the ticket stem"
    );
    std::fs::remove_dir_all(&root).ok();
}
