use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::ids::validate_identifier;

pub const MAX_EVENT_CHUNK_BYTES: usize = 64 * 1024;
const MAX_EVENT_REPLAY_WINDOW: u64 = 256;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionLifecycleV1 {
    Queued,
    Preparing,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventAcceptance {
    Accepted,
    Duplicate,
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

#[derive(Default)]
pub struct EventStreamValidator {
    execution_id: Option<String>,
    seen: BTreeMap<u64, ExecutionEventV1>,
    next_sequence: u64,
    lifecycle: Option<ExecutionLifecycleV1>,
    last_timestamp_unix_ms: u64,
}

impl EventStreamValidator {
    pub fn accept(&mut self, event: &ExecutionEventV1) -> anyhow::Result<EventAcceptance> {
        event.validate()?;
        if let Some(execution_id) = &self.execution_id {
            anyhow::ensure!(
                execution_id == &event.execution_id,
                "event execution ID changed"
            );
        }
        if let Some(previous) = self.seen.get(&event.sequence) {
            anyhow::ensure!(
                previous == event,
                "event sequence was reused with different data"
            );
            return Ok(EventAcceptance::Duplicate);
        }
        anyhow::ensure!(
            event.sequence >= self.next_sequence,
            "event replay is older than the deduplication window"
        );
        anyhow::ensure!(
            event.sequence == self.next_sequence.max(1),
            "event sequence is not contiguous"
        );
        let next_sequence = event
            .sequence
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("event sequence overflow"))?;
        anyhow::ensure!(
            event.timestamp_unix_ms >= self.last_timestamp_unix_ms,
            "event timestamp moved backwards"
        );
        self.validate_order(&event.event)?;
        if self.execution_id.is_none() {
            self.execution_id = Some(event.execution_id.clone());
        }
        self.last_timestamp_unix_ms = event.timestamp_unix_ms;
        self.seen.insert(event.sequence, event.clone());
        self.next_sequence = next_sequence;
        let first_retained = self.next_sequence.saturating_sub(MAX_EVENT_REPLAY_WINDOW);
        self.seen.retain(|sequence, _| *sequence >= first_retained);
        Ok(EventAcceptance::Accepted)
    }

    fn validate_order(&mut self, event: &ExecutionEventKindV1) -> anyhow::Result<()> {
        use ExecutionEventKindV1 as Event;
        if self.lifecycle.is_none() {
            anyhow::ensure!(
                matches!(
                    event,
                    Event::Lifecycle {
                        state: ExecutionLifecycleV1::Queued
                    }
                ),
                "execution event stream must start at queued"
            );
            self.lifecycle = Some(ExecutionLifecycleV1::Queued);
            return Ok(());
        }
        let Some(current) = self.lifecycle else {
            unreachable!()
        };
        anyhow::ensure!(
            !current.terminal(),
            "event appeared after a terminal lifecycle state"
        );
        match event {
            Event::Lifecycle { state } => {
                let allowed = matches!(
                    (current, state),
                    (
                        ExecutionLifecycleV1::Queued,
                        ExecutionLifecycleV1::Preparing
                    ) | (
                        ExecutionLifecycleV1::Preparing,
                        ExecutionLifecycleV1::Running
                    ) | (
                        ExecutionLifecycleV1::Running,
                        ExecutionLifecycleV1::Collecting
                    ) | (_, ExecutionLifecycleV1::Failed)
                        | (_, ExecutionLifecycleV1::Cancelled)
                        | (_, ExecutionLifecycleV1::Interrupted)
                        | (
                            ExecutionLifecycleV1::Collecting,
                            ExecutionLifecycleV1::Succeeded
                        )
                );
                anyhow::ensure!(allowed, "invalid execution lifecycle transition");
                self.lifecycle = Some(*state);
            }
            Event::Stdout { .. } | Event::Stderr { .. } => anyhow::ensure!(
                current == ExecutionLifecycleV1::Running,
                "output is only valid while execution is running"
            ),
            Event::Metrics { .. } => anyhow::ensure!(
                matches!(
                    current,
                    ExecutionLifecycleV1::Running | ExecutionLifecycleV1::Collecting
                ),
                "resource metrics arrived outside execution"
            ),
            Event::Artifact { .. } => anyhow::ensure!(
                current == ExecutionLifecycleV1::Collecting,
                "artifacts are only valid while collecting"
            ),
        }
        Ok(())
    }
}
