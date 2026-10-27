use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};

use super::super::{
    DlgHarnessSetup, ProbeView,
    probe_result::{from_harness_probe, internal_failure_result},
    probes::{PROVIDERS, discover_provider},
};
use super::ProbeMessage;

mod provider_queue;
use provider_queue::run_provider_workers;

static SCAN_GENERATION: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
#[path = "workers/tests.rs"]
mod tests;

fn run_provider_probe(
    provider: &str,
    generation: u64,
    started_at_unix_ms: u64,
    started: std::time::Instant,
    probe: impl FnOnce() -> super::super::HarnessProbe,
) -> (crate::harness::discovery::ProbeResult, u64) {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(probe));
    let elapsed_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
    let result = match outcome {
        Ok(report) => from_harness_probe(&report, generation, started_at_unix_ms, elapsed_ms),
        Err(_) => internal_failure_result(provider, generation, started_at_unix_ms, elapsed_ms),
    };
    (result, elapsed_ms)
}

impl DlgHarnessSetup {
    pub fn retry_provider(&mut self, id: &str) {
        if !PROVIDERS.contains(&id) {
            self.set_feedback(false, format!("Unsupported provider: {id}"));
            return;
        }
        if let Some(cancel) = &self.scan_cancel {
            cancel.store(true, Ordering::Release);
        }
        super::super::invalidate_discovery(&mut self.settings, Some(id));
        self.persist_quiet();
        let generation = SCAN_GENERATION.fetch_add(1, Ordering::Relaxed);
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let worker_cancel = cancel.clone();
        let provider = id.to_owned();
        let launch_stubbed = {
            #[cfg(test)]
            {
                super::test_hooks::retry_if_configured(generation, &provider)
            }
            #[cfg(not(test))]
            {
                false
            }
        };
        if !launch_stubbed {
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
                let (mut result, elapsed) =
                    run_provider_probe(&provider, generation, started_at_unix_ms, started, || {
                        crate::harness::discovery::with_cancellation(worker_cancel.clone(), || {
                            discover_provider(&provider, runtime_available)
                        })
                    });
                if !worker_cancel.load(Ordering::Acquire) {
                    let ready = result.status == crate::harness::discovery::ProbeStatus::Available;
                    crate::persistence::discovery_log::record_probe_result(
                        generation,
                        &provider,
                        elapsed,
                        ready,
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
        }
        self.probe_rx = Some(Arc::new(Mutex::new(rx)));
        self.scan_generation = generation;
        self.scan_cancel = Some(cancel);
        self.scan_results.clear();
        self.probe_results.remove(id);
        self.running_providers.clear();
        self.runtime_available = None;
        self.probe_view = ProbeView::Pending;
    }

    pub fn refresh(&mut self) {
        let (tx, rx) = mpsc::channel();
        let (generation, cancelled) = begin_scan_state(self);
        super::super::invalidate_discovery(&mut self.settings, None);
        self.persist_quiet();
        self.feedback = None;
        self.probe_rx = Some(Arc::new(Mutex::new(rx)));
        launch_scan_workers(generation, tx, cancelled);
    }
}

fn launch_scan_workers(
    generation: u64,
    tx: mpsc::Sender<ProbeMessage>,
    cancelled: Arc<AtomicBool>,
) {
    #[cfg(test)]
    if super::test_hooks::launch_if_configured(generation, tx.clone(), cancelled.clone()) {
        return;
    }
    let runtime_available = Arc::new(AtomicBool::new(false));
    let coordinator_tx = tx.clone();
    let worker_cancel = cancelled.clone();
    let worker_runtime = runtime_available.clone();
    std::thread::spawn(move || {
        let runtime_tx = coordinator_tx.clone();
        let runtime_state = runtime_available;
        let runtime_cancel = worker_cancel.clone();
        let runtime_probe = std::thread::spawn(move || {
            let available = crate::harness::discovery::with_cancellation(runtime_cancel, || {
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
        run_provider_workers(
            generation,
            coordinator_tx.clone(),
            worker_cancel,
            worker_runtime,
            Arc::new(discover_provider),
            Arc::new(|generation, id, elapsed, result| {
                let ready = result.status == crate::harness::discovery::ProbeStatus::Available;
                crate::persistence::discovery_log::record_probe_result(
                    generation,
                    id,
                    elapsed,
                    ready,
                    result
                        .failure_stage
                        .map(crate::harness::discovery::ProbeKind::key),
                    result
                        .diagnostic_code
                        .map(crate::harness::discovery::ProbeDiagnosticCode::label),
                );
            }),
        );
        let _ = runtime_probe.join();
        let _ = coordinator_tx.send(ProbeMessage::Finished { generation });
    });
}

pub(super) fn begin_scan_state(dialog: &mut DlgHarnessSetup) -> (u64, Arc<AtomicBool>) {
    if let Some(cancel) = &dialog.scan_cancel {
        cancel.store(true, Ordering::Release);
    }
    let generation = SCAN_GENERATION.fetch_add(1, Ordering::Relaxed);
    let cancelled = Arc::new(AtomicBool::new(false));
    dialog.scan_generation = generation;
    dialog.scan_cancel = Some(cancelled.clone());
    dialog.scan_results.clear();
    dialog.probe_results.clear();
    dialog.running_providers.clear();
    dialog.runtime_available = None;
    dialog.probe_view = ProbeView::Pending;
    dialog.feedback = None;
    (generation, cancelled)
}
