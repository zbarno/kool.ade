use crate::persistence::execution_records::DockerResourceUsageV1;

pub(super) fn merge_sample(
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
