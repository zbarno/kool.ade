use super::Queue;
use super::{
    PersistedQueue, QUEUE_VERSION, TaskQueueState, default_auto_build, default_auto_plan,
    default_parallel, from_current,
};
use crate::core::implementation::Failure;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize)]
struct PersistedQueueV2 {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(default = "default_auto_build")]
    auto_mode: bool,
    #[serde(default)]
    running: bool,
    #[serde(default = "default_parallel")]
    max_parallel: usize,
    #[serde(default)]
    last_error: String,
    #[serde(default)]
    recovery_paused: bool,
    #[serde(default)]
    tasks: BTreeMap<String, TaskQueueState>,
    #[serde(default)]
    legacy_tasks: BTreeMap<String, TaskQueueState>,
    #[serde(default)]
    task_identity_aliases: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PersistedQueueV3 {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(default = "default_auto_plan")]
    auto_plan: bool,
    #[serde(default = "default_auto_build")]
    auto_build: bool,
    #[serde(default)]
    auto_publish: bool,
    #[serde(default)]
    running: bool,
    #[serde(default = "default_parallel")]
    max_parallel: usize,
    #[serde(default)]
    last_error: String,
    #[serde(default)]
    recovery_paused: bool,
    #[serde(default)]
    tasks: BTreeMap<String, TaskQueueState>,
    #[serde(default)]
    legacy_tasks: BTreeMap<String, TaskQueueState>,
    #[serde(default)]
    task_identity_aliases: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(default)]
struct LegacyQueue {
    auto_mode: bool,
    running: bool,
    current_ticket: Option<String>,
    in_flight: BTreeSet<String>,
    max_parallel: usize,
    blocked: BTreeMap<String, serde_json::Value>,
    last_error: String,
    recovery_paused: bool,
    recovery_attempts: BTreeMap<String, usize>,
}

impl Default for LegacyQueue {
    fn default() -> Self {
        Self {
            auto_mode: true,
            running: false,
            current_ticket: None,
            in_flight: BTreeSet::new(),
            max_parallel: 3,
            blocked: BTreeMap::new(),
            last_error: String::new(),
            recovery_paused: false,
            recovery_attempts: BTreeMap::new(),
        }
    }
}

pub(in crate::core::implementation_queue::state) fn decode_value(
    value: serde_json::Value,
) -> anyhow::Result<(Queue, bool)> {
    let version = match value.get("schemaVersion") {
        None => 0,
        Some(version) => {
            let raw = version
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Task queue state version must be an integer"))?;
            anyhow::ensure!(
                raw <= u64::from(QUEUE_VERSION),
                "Unsupported task queue state version {raw}"
            );
            raw as u32
        }
    };
    if version == QUEUE_VERSION {
        let data: PersistedQueue = serde_json::from_value(value)?;
        let policy_repaired = data.auto_publish && !data.require_independent_checks;
        return Ok((from_current(data)?, policy_repaired));
    }
    if version == 4 {
        let mut current: PersistedQueue = serde_json::from_value(value)?;
        current.schema_version = QUEUE_VERSION;
        return Ok((from_current(current)?, true));
    }
    if version == 3 {
        let old: PersistedQueueV3 = serde_json::from_value(value)?;
        anyhow::ensure!(
            old.schema_version == 3,
            "Task queue schema version is inconsistent"
        );
        let current = PersistedQueue {
            schema_version: QUEUE_VERSION,
            auto_plan: old.auto_plan,
            auto_build: old.auto_build,
            auto_publish: old.auto_publish,
            require_independent_checks: false,
            running: old.running,
            max_parallel: old.max_parallel,
            last_error: old.last_error,
            recovery_paused: old.recovery_paused,
            tasks: old.tasks,
            legacy_tasks: old.legacy_tasks,
            task_identity_aliases: old.task_identity_aliases,
        };
        return Ok((from_current(current)?, true));
    }
    if version == 2 {
        let old: PersistedQueueV2 = serde_json::from_value(value)?;
        anyhow::ensure!(
            old.schema_version == 2,
            "Task queue schema version is inconsistent"
        );
        let current = PersistedQueue {
            schema_version: QUEUE_VERSION,
            auto_plan: default_auto_plan(),
            auto_build: old.auto_mode,
            // Earlier Auto mode combined queue continuation with publication.
            // Preserve build preference, but require an explicit new opt-in to publish.
            auto_publish: false,
            require_independent_checks: false,
            running: old.running,
            max_parallel: old.max_parallel,
            last_error: old.last_error,
            recovery_paused: old.recovery_paused,
            tasks: old.tasks,
            legacy_tasks: old.legacy_tasks,
            task_identity_aliases: old.task_identity_aliases,
        };
        return Ok((from_current(current)?, true));
    }
    let legacy: LegacyQueue = serde_json::from_value(value)?;
    let mut queue = Queue {
        auto_plan: default_auto_plan(),
        auto_build: legacy.auto_mode,
        auto_publish: false,
        require_independent_checks: false,
        running: legacy.running,
        current_ticket: legacy.current_ticket,
        in_flight: legacy.in_flight,
        max_parallel: legacy.max_parallel.clamp(1, 8),
        last_error: legacy.last_error,
        recovery_paused: legacy.recovery_paused,
        recovery_attempts: legacy.recovery_attempts,
        ..Queue::default()
    };
    for (path, value) in legacy.blocked {
        let failure = if version == 0 {
            Failure::from_legacy(
                value
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("Legacy task queue failure must be text"))?
                    .to_owned(),
            )
        } else {
            serde_json::from_value(value)?
        };
        queue.blocked.insert(path, failure);
    }
    Ok((queue, true))
}
