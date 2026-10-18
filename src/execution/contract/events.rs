use serde::{Deserialize, Serialize};

use super::ids::validate_identifier;

mod validator;
pub use validator::{EventAcceptance, EventStreamValidator};

#[cfg(test)]
#[path = "events/tests.rs"]
mod tests;

pub const MAX_EVENT_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionLifecycleV1 {
    Queued,
    Preparing,
    WaitingForDependencyApproval,
    WaitingForRuntime,
    Running,
    Collecting,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl ExecutionLifecycleV1 {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExecutionEventKindV1 {
    Lifecycle { state: ExecutionLifecycleV1 },
    Stdout { chunk: String },
    Stderr { chunk: String },
    Metrics { metrics: ResourceMetricsV1 },
    Artifact { artifact: ArtifactManifestEntryV1 },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionEventV1 {
    pub schema_version: u16,
    pub execution_id: String,
    pub sequence: u64,
    pub timestamp_unix_ms: u64,
    pub event: ExecutionEventKindV1,
}

impl ExecutionEventV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported execution event schema"
        );
        validate_identifier(&self.execution_id)?;
        anyhow::ensure!(self.sequence > 0, "event sequence must be positive");
        anyhow::ensure!(
            self.timestamp_unix_ms > 0,
            "event timestamp must be positive"
        );
        match &self.event {
            ExecutionEventKindV1::Stdout { chunk } | ExecutionEventKindV1::Stderr { chunk } => {
                anyhow::ensure!(
                    chunk.len() <= MAX_EVENT_CHUNK_BYTES,
                    "execution output event exceeds the chunk limit"
                );
            }
            ExecutionEventKindV1::Metrics { metrics } => metrics.validate()?,
            ExecutionEventKindV1::Artifact { artifact } => artifact.validate()?,
            _ => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResourceMetricsV1 {
    pub cpu_millis: u64,
    pub peak_memory_bytes: u64,
    pub process_count: u32,
    pub disk_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ArtifactManifestEntryV1 {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub media_type: String,
}

impl ArtifactManifestEntryV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        let path = std::path::Path::new(&self.relative_path);
        anyhow::ensure!(
            !path.is_absolute()
                && !self.relative_path.contains('\\')
                && !self.relative_path.chars().any(char::is_control)
                && path
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_))),
            "artifact path must be a normalized relative path"
        );
        anyhow::ensure!(
            self.size_bytes <= 1024 * 1024 * 1024,
            "artifact exceeds the size limit"
        );
        anyhow::ensure!(
            self.sha256.len() == 64 && self.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "artifact digest is invalid"
        );
        anyhow::ensure!(
            !self.media_type.is_empty() && self.media_type.len() <= 256,
            "artifact media type is invalid"
        );
        Ok(())
    }
}

impl ResourceMetricsV1 {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.cpu_millis <= 31_536_000_000_000
                && self.peak_memory_bytes <= 64 * 1024 * 1024 * 1024
                && self.process_count <= 4096
                && self.disk_bytes <= 100 * 1024 * 1024 * 1024,
            "execution resource metrics exceed supported limits"
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EventPageRequestV1 {
    pub after_sequence: u64,
    pub max_events: u16,
}

impl EventPageRequestV1 {
    pub fn new(after_sequence: u64, max_events: u16) -> anyhow::Result<Self> {
        anyhow::ensure!(
            (1..=256).contains(&max_events),
            "event page size is outside limits"
        );
        Ok(Self {
            after_sequence,
            max_events,
        })
    }
}
