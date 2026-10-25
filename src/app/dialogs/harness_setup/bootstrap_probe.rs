use crate::harness::discovery::ProbeResult;

use super::{probe_result, probes};

/// Reuse Coding Tools' bounded provider probes during first-run setup, before
/// Docker has been configured. Each provider runs independently; a failed or
/// panicking probe must not hide another installed CLI. This is discovery only:
/// it does not select a provider or authorize repository tools.
pub(crate) fn probe_bootstrap_clis(
    scan_generation: u64,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Vec<ProbeResult> {
    let mut results = Vec::with_capacity(probes::PROVIDERS.len());
    // Keep resource consumption bounded while preserving independent results.
    for batch in probes::PROVIDERS.chunks(3) {
        std::thread::scope(|scope| {
            let handles = batch
                .iter()
                .map(|&id| {
                    let cancellation = cancellation.clone();
                    scope.spawn(move || {
                        crate::harness::discovery::with_cancellation(cancellation.clone(), || {
                            let start = std::time::Instant::now();
                            let checked = crate::persistence::harness_settings::current_unix_ms();
                            if cancellation.load(std::sync::atomic::Ordering::Acquire) {
                                return probe_result::internal_failure_result(
                                    id,
                                    scan_generation,
                                    checked,
                                    0,
                                );
                            }
                            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                probes::discover_provider(id, false)
                            })) {
                                Ok(report) => probe_result::from_harness_probe(
                                    &report,
                                    scan_generation,
                                    checked,
                                    start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                                ),
                                Err(_) => probe_result::internal_failure_result(
                                    id,
                                    scan_generation,
                                    checked,
                                    start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
                                ),
                            }
                        })
                    })
                })
                .collect::<Vec<_>>();
            for (id, handle) in batch.iter().zip(handles) {
                results.push(handle.join().unwrap_or_else(|_| {
                    probe_result::internal_failure_result(
                        id,
                        scan_generation,
                        crate::persistence::harness_settings::current_unix_ms(),
                        0,
                    )
                }));
            }
        });
    }
    results
}
