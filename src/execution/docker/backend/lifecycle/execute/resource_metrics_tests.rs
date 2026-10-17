use super::{
    PERSIST_EVERY_SAMPLES, PERSIST_INTERVAL, merge_sample, persist_pending_samples,
    should_persist_sample, stream_retry_exhausted,
};
use crate::persistence::execution_records::DockerResourceUsageV1;
use std::{sync::Arc, time::Duration};

#[test]
fn stats_stream_retries_are_bounded_at_three_consecutive_failures() {
    assert!(!stream_retry_exhausted(0));
    assert!(!stream_retry_exhausted(2));
    assert!(stream_retry_exhausted(3));
    assert!(stream_retry_exhausted(u8::MAX));
}

#[test]
fn stats_records_are_batched_by_sample_count_or_elapsed_time() {
    assert!(!should_persist_sample(
        PERSIST_EVERY_SAMPLES - 1,
        0,
        PERSIST_INTERVAL / 2,
    ));
    assert!(should_persist_sample(
        PERSIST_EVERY_SAMPLES,
        0,
        PERSIST_INTERVAL / 2
    ));
    assert!(should_persist_sample(1, 0, PERSIST_INTERVAL));
    assert!(!should_persist_sample(2, 2, PERSIST_INTERVAL / 2));
}

#[test]
fn collector_stop_flushes_a_newer_sample_once() {
    let mut persisted = 8;
    let mut writes = 0;
    let flushed = persist_pending_samples(9, &mut persisted, || {
        writes += 1;
        Ok::<_, ()>(())
    })
    .unwrap();
    assert!(flushed);
    assert_eq!(persisted, 9);
    assert_eq!(writes, 1);

    let flushed = persist_pending_samples(9, &mut persisted, || {
        writes += 1;
        Ok::<_, ()>(())
    })
    .unwrap();
    assert!(!flushed);
    assert_eq!(writes, 1);
}

#[test]
fn failed_sample_persistence_does_not_acknowledge_or_hide_the_sample() {
    let mut persisted = 8;
    let error = persist_pending_samples(10, &mut persisted, || Err("disk full"));
    assert_eq!(error, Err("disk full"));
    assert_eq!(persisted, 8);

    let retried = persist_pending_samples(10, &mut persisted, || Ok::<_, &str>(()));
    assert_eq!(retried, Ok(true));
    assert_eq!(persisted, 10);
}

#[tokio::test]
async fn aborting_execution_owner_releases_metrics_reference_for_recovery() {
    let execution = Arc::new(());
    let weak_execution = Arc::downgrade(&execution);
    let task_execution = Arc::downgrade(&execution);
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let metrics_task = tokio::spawn(async move {
        let _execution = task_execution;
        let mut stop_rx = stop_rx;
        let _ = (&mut stop_rx).await;
    });
    drop(execution);

    // Dropping an aborted exec future closes its stop sender. The collector
    // can unwind without owning RuntimeExecution or its task lock.
    drop(stop_tx);
    tokio::time::timeout(Duration::from_secs(1), metrics_task)
        .await
        .expect("metrics collector should observe owner drop")
        .expect("metrics collector should exit cleanly");
    assert!(weak_execution.upgrade().is_none());
}

#[test]
fn linux_container_samples_accumulate_cpu_and_peak_memory_without_inventing_disk_usage() {
    let first = sample("linux", 25_000_000, 2048, 1024, 3);
    let usage = merge_sample(&DockerResourceUsageV1::default(), &first).unwrap();
    assert_eq!(usage.cpu_time_ms, Some(25));
    assert_eq!(usage.sampled_peak_memory_bytes, Some(2048));
    assert_eq!(usage.latest_process_count, Some(3));
    assert_eq!(usage.sample_count, 1);

    let later = sample("linux", 75_000_000, 4096, 2048, 4);
    let usage = merge_sample(&usage, &later).unwrap();
    assert_eq!(usage.cpu_time_ms, Some(75));
    assert_eq!(usage.sampled_peak_memory_bytes, Some(4096));
    assert_eq!(usage.latest_process_count, Some(4));
    assert_eq!(usage.sample_count, 2);

    let unchanged = merge_sample(&usage, &later).unwrap();
    assert_eq!(unchanged.sampled_peak_memory_bytes, Some(4096));
    assert_eq!(unchanged.sample_count, 3);
}

#[test]
fn unsupported_or_empty_samples_remain_unavailable() {
    let windows = sample("windows", 50_000_000, 1024, 512, 1);
    assert!(merge_sample(&DockerResourceUsageV1::default(), &windows).is_none());

    let empty: bollard::models::ContainerStatsResponse =
        serde_json::from_value(serde_json::json!({ "os_type": "linux" })).unwrap();
    assert!(merge_sample(&DockerResourceUsageV1::default(), &empty).is_none());
    assert_eq!(DockerResourceUsageV1::default().sample_count, 0);
}

fn sample(
    os_type: &str,
    cpu_nanoseconds: u64,
    memory_max_bytes: u64,
    memory_current_bytes: u64,
    process_count: u64,
) -> bollard::models::ContainerStatsResponse {
    serde_json::from_value(serde_json::json!({
        "os_type": os_type,
        "cpu_stats": { "cpu_usage": { "total_usage": cpu_nanoseconds } },
        "memory_stats": { "max_usage": memory_max_bytes, "usage": memory_current_bytes },
        "pids_stats": { "current": process_count }
    }))
    .unwrap()
}
