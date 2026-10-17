use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use bollard::query_parameters::StatsOptionsBuilder;
use futures_util::StreamExt;
use tokio::sync::oneshot;

use super::super::runtime::RuntimeExecution;
use crate::persistence::execution_records::DockerResourceUsageV1;

const MAX_RESOURCE_SAMPLES: u32 = 4096;
const MAX_CONSECUTIVE_STREAM_FAILURES: u8 = 3;
const STATS_RETRY_DELAY: Duration = Duration::from_millis(250);
const STATS_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);
const PERSIST_EVERY_SAMPLES: u32 = 8;
const PERSIST_INTERVAL: Duration = Duration::from_secs(5);

pub(super) async fn collect(
    docker: bollard::Docker,
    container_id: String,
    execution: std::sync::Weak<Mutex<RuntimeExecution>>,
    mut stop: oneshot::Receiver<()>,
) {
    let mut samples = docker.stats(
        &container_id,
        Some(StatsOptionsBuilder::default().stream(true).build()),
    );
    let mut consecutive_failures = 0_u8;
    let mut persisted_sample_count = 0_u32;
    let mut last_persisted_at = Instant::now();
    loop {
        let next = tokio::select! {
            _ = &mut stop => break,
            next = tokio::time::timeout(STATS_REQUEST_TIMEOUT, samples.next()) => next,
        };
        match next {
            Ok(Some(Ok(sample))) => {
                consecutive_failures = 0;
                let Some(execution) = execution.upgrade() else {
                    break;
                };
                let Ok(mut state) = execution.lock() else {
                    break;
                };
                if state.terminal || state.docker_usage.sample_count >= MAX_RESOURCE_SAMPLES {
                    break;
                }
                let Some(usage) = merge_sample(&state.docker_usage, &sample) else {
                    continue;
                };
                state.docker_usage = usage;
                if should_persist_sample(
                    state.docker_usage.sample_count,
                    persisted_sample_count,
                    last_persisted_at.elapsed(),
                ) {
                    if persist_pending_samples(
                        state.docker_usage.sample_count,
                        &mut persisted_sample_count,
                        || state.persist(),
                    )
                    .is_err()
                    {
                        break;
                    }
                    last_persisted_at = Instant::now();
                }
            }
            Ok(Some(Err(_))) | Ok(None) | Err(_) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                if stream_retry_exhausted(consecutive_failures) {
                    break;
                }
                // Docker stats streams can be interrupted during a daemon
                // reconnect. Retry a small bounded number of times, then leave
                // any partial measurements intact and stop sampling.
                tokio::select! {
                    _ = &mut stop => break,
                    _ = tokio::time::sleep(STATS_RETRY_DELAY) => {}
                }
                samples = docker.stats(
                    &container_id,
                    Some(StatsOptionsBuilder::default().stream(true).build()),
                );
            }
        }
    }
    // Always flush the latest sample when the execution stream stops. This
    // keeps terminal/restart recovery records current while avoiding a full
    // record rewrite for every Docker stats sample during long executions.
    if let Some(execution) = execution.upgrade()
        && let Ok(state) = execution.lock()
    {
        let _ = persist_pending_samples(
            state.docker_usage.sample_count,
            &mut persisted_sample_count,
            || state.persist(),
        );
    }
}

fn persist_pending_samples<E>(
    sample_count: u32,
    persisted_sample_count: &mut u32,
    persist: impl FnOnce() -> Result<(), E>,
) -> Result<bool, E> {
    if sample_count <= *persisted_sample_count {
        return Ok(false);
    }
    persist()?;
    *persisted_sample_count = sample_count;
    Ok(true)
}

fn should_persist_sample(
    sample_count: u32,
    persisted_sample_count: u32,
    elapsed: Duration,
) -> bool {
    (persisted_sample_count == 0 && sample_count > 0)
        || sample_count.saturating_sub(persisted_sample_count) >= PERSIST_EVERY_SAMPLES
        || elapsed >= PERSIST_INTERVAL
}

fn stream_retry_exhausted(consecutive_failures: u8) -> bool {
    consecutive_failures >= MAX_CONSECUTIVE_STREAM_FAILURES
}

fn merge_sample(
    previous: &DockerResourceUsageV1,
    sample: &bollard::models::ContainerStatsResponse,
) -> Option<DockerResourceUsageV1> {
    if sample.os_type.as_deref() != Some("linux") {
        return None;
    }
    let mut current = previous.clone();
    if let Some(cpu_time_ms) = sample
        .cpu_stats
        .as_ref()
        .and_then(|stats| stats.cpu_usage.as_ref())
        .and_then(|usage| usage.total_usage)
        .map(|nanoseconds| nanoseconds / 1_000_000)
    {
        current.cpu_time_ms = Some(current.cpu_time_ms.unwrap_or_default().max(cpu_time_ms));
    }
    if let Some(memory_bytes) = sample
        .memory_stats
        .as_ref()
        .and_then(|stats| stats.max_usage.or(stats.usage))
    {
        current.sampled_peak_memory_bytes = Some(
            current
                .sampled_peak_memory_bytes
                .unwrap_or_default()
                .max(memory_bytes),
        );
    }
    if let Some(processes) = sample
        .pids_stats
        .as_ref()
        .and_then(|stats| stats.current)
        .and_then(|current| u32::try_from(current).ok())
    {
        current.latest_process_count = Some(processes);
    }
    if current.cpu_time_ms.is_none()
        && current.sampled_peak_memory_bytes.is_none()
        && current.latest_process_count.is_none()
    {
        return None;
    }
    current.sample_count = previous.sample_count.saturating_add(1);
    current.validate().then_some(current)
}

#[cfg(test)]
#[path = "resource_metrics_tests.rs"]
mod tests;
