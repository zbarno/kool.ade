use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    mpsc,
};

use super::{
    DlgHarnessSetup, HarnessProbe, ProbeView,
    probes::{PROVIDERS, discover_provider},
};

mod results;
pub(super) use results::apply_probe_results;
#[cfg(test)]
pub(super) use results::update_runtime_readiness;
use results::{failed_probe, update_settings_from_probe};

static SCAN_GENERATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug)]
pub(super) enum ProbeMessage {
    Provider {
        generation: u64,
        report: HarnessProbe,
    },
    Runtime {
        generation: u64,
        implementation_available: bool,
    },
    Finished {
        generation: u64,
    },
}

impl DlgHarnessSetup {
    pub fn cancel_discovery(&mut self) {
        if let Some(cancel) = self.scan_cancel.take() {
            cancel.store(true, Ordering::Release);
        }
        self.probe_rx = None;
        self.probe_view = ProbeView::Complete;
    }

    pub fn retry_provider(&mut self, id: &str) {
        if !PROVIDERS.contains(&id) {
            self.feedback = Some((false, format!("Unsupported provider: {id}")));
            return;
        }
        if let Some(cancel) = &self.scan_cancel {
            cancel.store(true, Ordering::Release);
        }
        super::invalidate_discovery(&mut self.settings, Some(id));
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
            let started = std::time::Instant::now();
            let report = std::panic::catch_unwind(|| {
                crate::harness::discovery::with_cancellation(worker_cancel.clone(), || {
                    discover_provider(&provider, runtime_available)
                })
            })
            .unwrap_or_else(|_| failed_probe(&provider, "Provider discovery failed unexpectedly"));
            if !worker_cancel.load(Ordering::Acquire) {
                crate::persistence::discovery_log::record_probe(
                    generation,
                    &provider,
                    started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                    report.ready,
                );
            }
            if !worker_cancel.load(Ordering::Acquire) {
                let _ = tx.send(ProbeMessage::Provider { generation, report });
                let _ = tx.send(ProbeMessage::Finished { generation });
            }
        });
        self.probe_rx = Some(Arc::new(Mutex::new(rx)));
        self.scan_generation = generation;
        self.scan_cancel = Some(cancel);
        self.scan_results.clear();
        self.runtime_available = None;
        self.feedback = Some((true, format!("Refreshing {id} discovery…")));
    }

    pub fn refresh(&mut self) {
        let (tx, rx) = mpsc::channel();
        if let Some(cancel) = &self.scan_cancel {
            cancel.store(true, Ordering::Release);
        }
        super::invalidate_discovery(&mut self.settings, None);
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
                        let started = std::time::Instant::now();
                        let report = std::panic::catch_unwind(|| {
                            crate::harness::discovery::with_cancellation(cancel.clone(), || {
                                discover_provider(id, runtime.load(Ordering::Acquire))
                            })
                        })
                        .unwrap_or_else(|_| failed_probe(id, "Probe crashed unexpectedly"));
                        if !cancel.load(Ordering::Acquire) {
                            crate::persistence::discovery_log::record_probe(
                                generation,
                                id,
                                started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                                report.ready,
                            );
                        }
                        if !cancel.load(Ordering::Acquire)
                            && tx
                                .send(ProbeMessage::Provider { generation, report })
                                .is_err()
                        {
                            break;
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
        self.runtime_available = None;
        self.probe_view = ProbeView::Pending;
        self.feedback = None;
    }

    pub fn drain_probe(&mut self) {
        let Some(rx) = self.probe_rx.as_ref().cloned() else {
            return;
        };
        loop {
            let result = match rx.lock() {
                Ok(rx) => rx.try_recv(),
                Err(_) => Err(mpsc::TryRecvError::Disconnected),
            };
            match result {
                Ok(message) => self.apply_probe_message(message),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.probe_rx = None;
                    self.feedback = Some((false, "Harness discovery stopped unexpectedly.".into()));
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
    }

    pub(super) fn apply_probe_message(&mut self, message: ProbeMessage) {
        match message {
            ProbeMessage::Provider { generation, report } if generation == self.scan_generation => {
                let mut report = report;
                if let Some(runtime_available) = self.runtime_available {
                    report.implementation_available = report.ready && runtime_available;
                }
                update_settings_from_probe(&mut self.settings, &report, generation);
                if let Some(runtime_available) = self.runtime_available
                    && let Some(entry) = self.settings.discovered.get_mut(&report.id)
                    && let Some(snapshot) = entry.capability_snapshot.as_mut()
                {
                    snapshot.execution_target = if runtime_available {
                        crate::execution::contract::ReadinessState::Ready
                    } else {
                        crate::execution::contract::ReadinessState::Unavailable
                    };
                    entry.implementation_available = report.ready && runtime_available;
                }
                self.scan_results.push(report);
                self.persist();
            }
            ProbeMessage::Runtime {
                generation,
                implementation_available,
            } if generation == self.scan_generation => {
                self.runtime_available = Some(implementation_available);
                results::update_runtime_readiness(
                    &mut self.settings,
                    &mut self.scan_results,
                    generation,
                    implementation_available,
                );
                for entry in self.settings.discovered.values_mut() {
                    entry.implementation_available = entry.ready && implementation_available;
                }
                self.persist();
            }
            ProbeMessage::Finished { generation } if generation == self.scan_generation => {
                apply_probe_results(
                    &mut self.settings,
                    &self.scan_results,
                    std::env::var(crate::harness::CODEX_HARNESS_ENV)
                        .ok()
                        .as_deref(),
                    generation,
                );
                self.persist();
                self.probe_rx = None;
                self.scan_cancel = None;
                self.probe_view = ProbeView::Complete;
            }
            _ => {}
        }
    }
}
