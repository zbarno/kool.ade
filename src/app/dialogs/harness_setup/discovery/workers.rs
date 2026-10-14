use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    mpsc,
};

use super::super::{
    DlgHarnessSetup, ProbeView,
    probe_result::from_harness_probe,
    probes::{PROVIDERS, discover_provider},
};
use super::{ProbeMessage, results::failed_probe};

static SCAN_GENERATION: AtomicU64 = AtomicU64::new(1);

impl DlgHarnessSetup {
    pub fn retry_provider(&mut self, id: &str) {
        if !PROVIDERS.contains(&id) {
            self.feedback = Some((false, format!("Unsupported provider: {id}")));
            return;
        }
        if let Some(cancel) = &self.scan_cancel {
            cancel.store(true, Ordering::Release);
        }
        super::super::invalidate_discovery(&mut self.settings, Some(id));
        self.persist();
        let generation = SCAN_GENERATION.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let worker_cancel = cancel.clone();
        let provider = id.to_owned();
        std::thread::spawn(move || {
            let runtime_available =
                crate::harness::discovery::with_cancellation(worker_cancel.clone(), || {
                    std::panic::catch_unwind(
                        crate::harness::runtime_capabilities::RuntimeCapabilities::detect,
                    )
                    .map(|capabilities| capabilities.implementation)
                    .unwrap_or(false)
                });
            if worker_cancel.load(Ordering::Acquire) {
                return;
            }
            let _ = tx.send(ProbeMessage::Runtime {
                generation,
                implementation_available: runtime_available,
            });
            let _ = tx.send(ProbeMessage::Started {
                generation,
                provider_id: provider.clone(),
            });
            let started_at_unix_ms = crate::persistence::harness_settings::current_unix_ms();
            let started = std::time::Instant::now();
            let report = std::panic::catch_unwind(|| {
                crate::harness::discovery::with_cancellation(worker_cancel.clone(), || {
                    discover_provider(&provider, runtime_available)
                })
            })
            .unwrap_or_else(|_| failed_probe(&provider, "Provider discovery failed unexpectedly"));
            let elapsed = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
            if !worker_cancel.load(Ordering::Acquire) {
                let mut result =
                    from_harness_probe(&report, generation, started_at_unix_ms, elapsed);
                crate::persistence::discovery_log::record_probe_result(
                    generation,
                    &provider,
                    elapsed,
                    report.ready,
                    result
                        .failure_stage
                        .map(crate::harness::discovery::ProbeKind::key),
                    result
                        .diagnostic_code
                        .map(crate::harness::discovery::ProbeDiagnosticCode::label),
                );
                result.set_runtime_readiness(runtime_available);
                let _ = tx.send(ProbeMessage::Provider { generation, result });
                let _ = tx.send(ProbeMessage::Finished { generation });
            }
        });
        self.probe_rx = Some(Arc::new(Mutex::new(rx)));
        self.scan_generation = generation;
        self.scan_cancel = Some(cancel);
        self.scan_results.clear();
        self.probe_results.remove(id);
        self.running_providers.clear();
        self.runtime_available = None;
        self.probe_view = ProbeView::Pending;
        self.feedback = Some((true, format!("Refreshing {id} discovery…")));
    }

    pub fn refresh(&mut self) {
        let (tx, rx) = mpsc::channel();
        if let Some(cancel) = &self.scan_cancel {
            cancel.store(true, Ordering::Release);
        }
        super::super::invalidate_discovery(&mut self.settings, None);
        self.persist();
        let generation = SCAN_GENERATION.fetch_add(1, Ordering::Relaxed);
        let cancelled = Arc::new(AtomicBool::new(false));
        let runtime_available = Arc::new(AtomicBool::new(false));
        let coordinator_tx = tx.clone();
        let worker_cancel = cancelled.clone();
        let worker_runtime = runtime_available.clone();
        std::thread::spawn(move || {
            let runtime_tx = coordinator_tx.clone();
            let runtime_state = runtime_available;
            let runtime_cancel = worker_cancel.clone();
            let runtime_probe = std::thread::spawn(move || {
                let available =
                    crate::harness::discovery::with_cancellation(runtime_cancel, || {
                        std::panic::catch_unwind(
                            crate::harness::runtime_capabilities::RuntimeCapabilities::detect,
                        )
                        .map(|capabilities| capabilities.implementation)
                        .unwrap_or(false)
                    });
                runtime_state.store(available, Ordering::Release);
                let _ = runtime_tx.send(ProbeMessage::Runtime {
                    generation,
                    implementation_available: available,
                });
            });
            let queue = Arc::new(AtomicUsize::new(0));
            let mut workers = Vec::new();
            for _ in 0..3 {
                let tx = coordinator_tx.clone();
                let queue = queue.clone();
                let cancel = worker_cancel.clone();
                let runtime = worker_runtime.clone();
                workers.push(std::thread::spawn(move || {
                    loop {
                        if cancel.load(Ordering::Acquire) {
                            break;
                        }
                        let index = queue.fetch_add(1, Ordering::Relaxed);
                        let Some(id) = PROVIDERS.get(index) else {
                            break;
                        };
                        if tx
                            .send(ProbeMessage::Started {
                                generation,
                                provider_id: (*id).to_owned(),
                            })
                            .is_err()
                        {
                            break;
                        }
                        let started_at_unix_ms =
                            crate::persistence::harness_settings::current_unix_ms();
                        let started = std::time::Instant::now();
                        let report = std::panic::catch_unwind(|| {
                            crate::harness::discovery::with_cancellation(cancel.clone(), || {
                                discover_provider(id, runtime.load(Ordering::Acquire))
                            })
                        })
                        .unwrap_or_else(|_| failed_probe(id, "Probe crashed unexpectedly"));
                        let elapsed =
                            started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                        if !cancel.load(Ordering::Acquire) {
                            let result = from_harness_probe(
                                &report,
                                generation,
                                started_at_unix_ms,
                                elapsed,
                            );
                            crate::persistence::discovery_log::record_probe_result(
                                generation,
                                id,
                                elapsed,
                                report.ready,
                                result
                                    .failure_stage
                                    .map(crate::harness::discovery::ProbeKind::key),
                                result
                                    .diagnostic_code
                                    .map(crate::harness::discovery::ProbeDiagnosticCode::label),
                            );
                            if tx
                                .send(ProbeMessage::Provider { generation, result })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }));
            }
            for worker in workers {
                let _ = worker.join();
            }
            let _ = runtime_probe.join();
            let _ = coordinator_tx.send(ProbeMessage::Finished { generation });
        });
        self.probe_rx = Some(Arc::new(Mutex::new(rx)));
        self.scan_generation = generation;
        self.scan_cancel = Some(cancelled);
        self.scan_results.clear();
        self.probe_results.clear();
        self.running_providers.clear();
        self.runtime_available = None;
        self.probe_view = ProbeView::Pending;
        self.feedback = None;
    }
}
