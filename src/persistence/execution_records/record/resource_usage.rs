use serde::{Deserialize, Serialize};

/// Bounded measurements collected from Docker's stats API for this container.
/// Missing values indicate that the daemon did not provide that measurement.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DockerResourceUsageV1 {
    #[serde(default)]
    pub cpu_time_ms: Option<u64>,
    #[serde(default)]
    pub sampled_peak_memory_bytes: Option<u64>,
    #[serde(default)]
    pub latest_process_count: Option<u32>,
    #[serde(default)]
    pub sample_count: u32,
}

impl DockerResourceUsageV1 {
    pub(crate) fn validate(&self) -> bool {
        self.cpu_time_ms
            .is_none_or(|value| value <= 31_536_000_000_000)
            && self
                .sampled_peak_memory_bytes
                .is_none_or(|value| value <= 64 * 1024 * 1024 * 1024)
            && self.latest_process_count.is_none_or(|value| value <= 4096)
            && self.sample_count <= 4096
    }
}
