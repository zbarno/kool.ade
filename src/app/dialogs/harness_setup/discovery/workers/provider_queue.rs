use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};

use super::{ProbeMessage, run_provider_probe};
use crate::app::dialogs::harness_setup::probes::PROVIDERS;
use crate::harness::discovery::ProbeResult;

pub(super) type ProviderDiscovery =
    dyn Fn(&str, bool) -> super::super::super::HarnessProbe + Send + Sync;
type ProbeLogger = dyn Fn(u64, &str, u64, &ProbeResult) + Send + Sync;

pub(super) fn run_provider_workers(
    generation: u64,
    tx: mpsc::Sender<ProbeMessage>,
    cancelled: Arc<AtomicBool>,
    runtime_available: Arc<AtomicBool>,
    discover: Arc<ProviderDiscovery>,
    log: Arc<ProbeLogger>,
) {
    let queue = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();
    for _ in 0..3 {
        let tx = tx.clone();
        let queue = queue.clone();
        let cancel = cancelled.clone();
        let runtime = runtime_available.clone();
        let discover = discover.clone();
        let log = log.clone();
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
                let started_at = crate::persistence::harness_settings::current_unix_ms();
                let started = std::time::Instant::now();
                let (result, elapsed) =
                    run_provider_probe(id, generation, started_at, started, || {
                        crate::harness::discovery::with_cancellation(cancel.clone(), || {
                            discover(id, runtime.load(Ordering::Acquire))
                        })
                    });
                if cancel.load(Ordering::Acquire) {
                    continue;
                }
                log(generation, id, elapsed, &result);
                if tx
                    .send(ProbeMessage::Provider { generation, result })
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
}
