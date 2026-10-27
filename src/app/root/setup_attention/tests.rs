use super::super::KooladeApp;
use super::ProbeOutput;
use crate::app::setup_attention::{SetupIssue, SetupScanResult, SetupScanVerification};

fn empty_scan(generation: u64, cancelled: bool) -> SetupScanResult {
    SetupScanResult {
        generation,
        issue: None,
        snapshot: None,
        endpoint_fingerprint: None,
        daemon_identity_fingerprint: None,
        docker_readiness_fingerprint: None,
        setup_provider: None,
        selected_provider: None,
        cancelled,
    }
}

#[test]
fn pending_refresh_discards_a_stale_scan_before_accepting_its_result() {
    let mut app = KooladeApp {
        setup_refresh_pending: Some(true),
        ..KooladeApp::default()
    };
    app.setup_attention
        .push(SetupIssue::provider("Stale machine readiness result"));

    let rerun = app.finish_setup_probe_result(SetupScanVerification {
        result: empty_scan(2, false),
        machine_setup_tasks: None,
        persistence_failed: false,
    });

    assert_eq!(rerun, Some(true));
    assert!(app.setup_attention.is_empty());
    assert!(app.setup_refresh_pending.is_none());
    assert!(app.setup_retry_requested);
}

#[test]
fn canceled_stale_generation_does_not_start_an_unrequested_retry() {
    let mut app = KooladeApp::default();
    app.setup_attention
        .push(SetupIssue::provider("Stale machine readiness result"));

    let rerun = app.finish_setup_probe_result(SetupScanVerification {
        result: empty_scan(2, true),
        machine_setup_tasks: None,
        persistence_failed: false,
    });

    assert_eq!(rerun, None);
    assert!(app.setup_attention.is_empty());
    assert!(!app.setup_retry_requested);
}

#[test]
fn refresh_defers_cancellation_while_commit_is_held_and_coalesces_retry_intent() {
    let mut app = KooladeApp::default();
    let cancellation = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.setup_probe_cancel = Some(cancellation.clone());
    app.setup_probe = Some(std::thread::spawn(|| {
        ProbeOutput::Scanned(empty_scan(0, false))
    }));

    let setup_commit_lock = app.setup_commit_lock.clone();
    let commit_guard = setup_commit_lock.lock().unwrap();
    app.refresh_setup_attention(true);
    assert!(!cancellation.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(app.setup_refresh_pending, Some(true));
    drop(commit_guard);

    let result = match app.setup_probe.take().unwrap().join().unwrap() {
        ProbeOutput::Scanned(result) => result,
        ProbeOutput::Verified(_) => panic!("unexpected verification result"),
    };
    let rerun = app.finish_setup_probe_result(SetupScanVerification {
        result,
        machine_setup_tasks: None,
        persistence_failed: false,
    });
    assert_eq!(rerun, Some(true));
    assert!(app.setup_refresh_pending.is_none());
}
