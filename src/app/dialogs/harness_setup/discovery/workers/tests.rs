use super::super::ProbeMessage;
use super::run_provider_probe;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};

fn synthetic_probe(id: &str, ready: bool) -> super::super::super::HarnessProbe {
    super::super::super::HarnessProbe {
        id: id.into(),
        status: if ready {
            format!("{id} 1.0")
        } else {
            format!("{id} (unavailable)")
        },
        version: ready.then(|| "1.0".into()),
        executable: ready.then(|| format!("/synthetic/{id}")),
        diagnostic: (!ready).then(|| "synthetic provider-local failure".into()),
        ready,
        models: ready
            .then(|| "synthetic-model".into())
            .into_iter()
            .collect(),
        default_model: ready.then(|| "synthetic-model".into()),
        model_catalog_fresh: ready,
        configuration_required: false,
        authentication: crate::execution::contract::ReadinessState::Ready,
        implementation_available: false,
    }
}

#[test]
fn provider_panic_becomes_a_provider_local_internal_failure() {
    let started = std::time::Instant::now();
    let (result, elapsed) = run_provider_probe("codex", 19, 123, started, || {
        panic!("synthetic provider panic")
    });

    assert_eq!(result.provider_id, "codex");
    assert_eq!(result.run_generation, 19);
    assert_eq!(result.started_at_unix_ms, 123);
    assert_eq!(
        result.status,
        crate::harness::discovery::ProbeStatus::Failed
    );
    assert_eq!(
        result.diagnostic_code,
        Some(crate::harness::discovery::ProbeDiagnosticCode::InternalProbeFailure)
    );
    assert_eq!(
        result.failure_stage,
        Some(crate::harness::discovery::ProbeKind::Capability)
    );
    assert!(elapsed < 1_000);
}

#[test]
fn repeated_scan_start_cancels_the_prior_generation_and_resets_current_progress() {
    let mut dialog = super::super::super::DlgHarnessSetup {
        settings: Default::default(),
        probe_view: super::super::super::ProbeView::Complete,
        feedback: None,
        section: Default::default(),
        manual_path_drafts: Default::default(),
        probe_rx: None,
        scan_generation: 0,
        scan_cancel: None,
        runtime_available: Some(true),
        scan_results: vec![synthetic_probe("pi", true)],
        probe_results: Default::default(),
        running_providers: std::collections::BTreeSet::from(["pi".into()]),
    };

    let (first_generation, first_cancel) = super::begin_scan_state(&mut dialog);
    let (second_generation, second_cancel) = super::begin_scan_state(&mut dialog);

    assert!(second_generation > first_generation);
    assert!(first_cancel.load(Ordering::Acquire));
    assert!(!second_cancel.load(Ordering::Acquire));
    assert_eq!(dialog.scan_generation, second_generation);
    assert_eq!(dialog.probe_view, super::super::super::ProbeView::Pending);
    assert!(dialog.runtime_available.is_none());
    assert!(dialog.scan_results.is_empty());
    assert!(dialog.probe_results.is_empty());
    assert!(dialog.running_providers.is_empty());
    second_cancel.store(true, Ordering::Release);
}

#[test]
fn provider_queue_reports_all_providers_after_local_failure_and_panic() {
    struct Active(Arc<AtomicUsize>);
    impl Drop for Active {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::AcqRel);
        }
    }

    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let discover: Arc<super::provider_queue::ProviderDiscovery> = {
        let active = active.clone();
        let peak = peak.clone();
        let calls = calls.clone();
        Arc::new(move |id: &str, _runtime| {
            calls.fetch_add(1, Ordering::Relaxed);
            let now = active.fetch_add(1, Ordering::AcqRel) + 1;
            peak.fetch_max(now, Ordering::AcqRel);
            let _active = Active(active.clone());
            std::thread::sleep(std::time::Duration::from_millis(10));
            if id == "codex" {
                panic!("synthetic codex panic");
            }
            synthetic_probe(id, id != "pi")
        })
    };
    let (tx, rx) = mpsc::channel();
    super::provider_queue::run_provider_workers(
        23,
        tx,
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
        discover,
        Arc::new(|_, _, _, _| {}),
    );

    let results = rx
        .into_iter()
        .filter_map(|message| match message {
            ProbeMessage::Provider { result, .. } => Some(result),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut ids = results
        .iter()
        .map(|result| result.provider_id.as_str())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    assert_eq!(
        ids,
        [
            "antigravity",
            "claude",
            "codex",
            "copilot",
            "opencode",
            "pi"
        ]
    );
    assert_eq!(calls.load(Ordering::Acquire), 6);
    assert!(peak.load(Ordering::Acquire) <= 3);
    assert!(peak.load(Ordering::Acquire) > 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| result.status == crate::harness::discovery::ProbeStatus::Available)
            .count(),
        3
    );
    assert!(results.iter().any(|result| {
        result.provider_id == "claude"
            && result.status == crate::harness::discovery::ProbeStatus::Installed
    }));
    assert!(results.iter().any(|result| {
        result.provider_id == "codex"
            && result.diagnostic_code
                == Some(crate::harness::discovery::ProbeDiagnosticCode::InternalProbeFailure)
    }));
}
