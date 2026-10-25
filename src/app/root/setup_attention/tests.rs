use super::super::KooladeApp;
use super::ProbeOutput;

#[test]
fn stale_scan_result_is_discarded_before_local_task_reconciliation() {
    let mut app = KooladeApp::default();
    app.setup_scan_generation = 3;
    app.setup_refresh_pending = Some(true);
    app.setup_attention = Some(crate::app::setup_attention::SetupIssue::provider(
        "Stale machine readiness result",
    ));
    app.machine_setup_tasks
        .push(crate::persistence::setup_tasks::LocalTask {
            id: "setup:images:general:amd64".into(),
            title: "Stale task projection".into(),
            explanation: "Must be cleared before a retry".into(),
            next_step: "Rescan".into(),
            prerequisites: Vec::new(),
            state: crate::persistence::setup_tasks::State::NeedsAttention,
            observed_at_unix_ms: 1,
            verified_at_unix_ms: None,
            events: Vec::new(),
        });
    app.setup_probe_cancel = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
        false,
    )));
    let result = crate::app::setup_attention::SetupScanResult {
        generation: 2,
        issue: None,
        snapshot: None,
        endpoint_fingerprint: None,
        daemon_identity_fingerprint: None,
        docker_readiness_fingerprint: None,
        selected_provider: None,
        cancelled: false,
    };

    let rerun = app.finish_setup_probe_result(crate::app::setup_attention::SetupScanVerification {
        result,
        machine_setup_tasks: None,
        persistence_failed: false,
    });

    assert_eq!(rerun, Some(true));
    assert!(app.setup_attention.is_none());
    assert!(app.machine_setup_tasks.is_empty());
    assert!(app.setup_refresh_pending.is_none());
    assert!(app.setup_retry_requested);
}

#[test]
fn refresh_defers_cancellation_while_commit_is_held_and_coalesces_retry_intent() {
    let mut app = KooladeApp::default();
    let cancellation = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    app.setup_probe_cancel = Some(cancellation.clone());
    app.setup_probe = Some(std::thread::spawn(|| {
        ProbeOutput::Scanned(crate::app::setup_attention::SetupScanResult {
            generation: 0,
            issue: None,
            snapshot: None,
            endpoint_fingerprint: None,
            daemon_identity_fingerprint: None,
            docker_readiness_fingerprint: None,
            selected_provider: None,
            cancelled: false,
        })
    }));

    let setup_commit_lock = app.setup_commit_lock.clone();
    let _commit_guard = setup_commit_lock.lock().unwrap();
    app.refresh_setup_attention(true);
    assert!(!cancellation.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(app.setup_scan_generation, 0);
    assert_eq!(app.setup_refresh_pending, Some(true));
    drop(_commit_guard);
    let result = match app.setup_probe.take().unwrap().join().unwrap() {
        ProbeOutput::Scanned(result) => result,
        ProbeOutput::Verified(_) => panic!("unexpected verification result"),
    };
    let rerun = app.finish_setup_probe_result(crate::app::setup_attention::SetupScanVerification {
        result,
        machine_setup_tasks: None,
        persistence_failed: false,
    });
    assert_eq!(rerun, Some(true));
    assert_eq!(app.setup_scan_generation, 1);
    assert!(app.setup_refresh_pending.is_none());
}
