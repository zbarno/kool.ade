use super::Queue;
use crate::core::implementation::Failure;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::collections::{BTreeMap, BTreeSet};

const QUEUE_VERSION: u32 = 4;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct TaskQueueState {
    #[serde(default)]
    pub(super) path_hint: String,
    #[serde(default)]
    pub(super) current: bool,
    #[serde(default)]
    pub(super) in_flight: bool,
    #[serde(default)]
    pub(super) blocked: Option<Failure>,
    #[serde(default)]
    pub(super) recovery_attempts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedQueue {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    #[serde(default = "default_auto_plan")]
    auto_plan: bool,
    #[serde(default = "default_auto_build")]
    auto_build: bool,
    #[serde(default)]
    auto_publish: bool,
    #[serde(default)]
    require_independent_checks: bool,
    #[serde(default)]
    running: bool,
    #[serde(default = "default_parallel")]
    max_parallel: usize,
    #[serde(default)]
    last_error: String,
    #[serde(default)]
    recovery_paused: bool,
    #[serde(default)]
    pub(super) tasks: BTreeMap<String, TaskQueueState>,
    #[serde(default)]
    legacy_tasks: BTreeMap<String, TaskQueueState>,
    #[serde(default)]
    task_identity_aliases: BTreeMap<String, String>,
}

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

fn default_auto_plan() -> bool {
    true
}
fn default_auto_build() -> bool {
    true
}
fn default_parallel() -> usize {
    3
}

impl Serialize for Queue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        persisted(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Queue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        decode_value(value)
            .map(|(queue, _)| queue)
            .map_err(D::Error::custom)
    }
}

pub(super) fn persisted(queue: &Queue) -> PersistedQueue {
    let mut tasks = queue
        .stable_tasks
        .iter()
        .filter(|(_, state)| has_state(state))
        .map(|(uid, state)| (uid.clone(), state.clone()))
        .collect::<BTreeMap<_, _>>();
    for uid in queue.current_paths.keys() {
        tasks.remove(uid);
    }
    let mut legacy_tasks = BTreeMap::new();
    let mut paths = queue.in_flight.iter().cloned().collect::<BTreeSet<_>>();
    paths.extend(queue.blocked.keys().cloned());
    paths.extend(queue.recovery_attempts.keys().cloned());
    paths.extend(queue.current_ticket.iter().cloned());
    for path in paths {
        let state = TaskQueueState {
            path_hint: path.clone(),
            current: queue.current_ticket.as_deref() == Some(path.as_str()),
            in_flight: queue.in_flight.contains(&path),
            blocked: queue.blocked.get(&path).cloned(),
            recovery_attempts: queue
                .recovery_attempts
                .get(&path)
                .copied()
                .unwrap_or_default(),
        };
        if let Some(uid) = queue.task_uids.get(&path) {
            tasks.insert(uid.clone(), state);
        } else {
            legacy_tasks.insert(path, state);
        }
    }
    PersistedQueue {
        schema_version: QUEUE_VERSION,
        auto_plan: queue.auto_plan,
        auto_build: queue.auto_build,
        auto_publish: queue.auto_publish,
        require_independent_checks: queue.require_independent_checks,
        running: queue.running,
        max_parallel: queue.max_parallel,
        last_error: queue.last_error.clone(),
        recovery_paused: queue.recovery_paused,
        tasks,
        legacy_tasks,
        task_identity_aliases: queue.task_uids.clone(),
    }
}

fn has_state(state: &TaskQueueState) -> bool {
    state.current || state.in_flight || state.blocked.is_some() || state.recovery_attempts > 0
}

fn from_current(data: PersistedQueue) -> anyhow::Result<Queue> {
    anyhow::ensure!(
        data.schema_version == QUEUE_VERSION,
        "Task queue schema version is inconsistent"
    );
    let mut queue = Queue {
        auto_plan: data.auto_plan,
        auto_build: data.auto_build,
        auto_publish: data.auto_publish,
        require_independent_checks: data.require_independent_checks || data.auto_publish,
        running: data.running,
        max_parallel: data.max_parallel.clamp(1, 8),
        last_error: data.last_error,
        recovery_paused: data.recovery_paused,
        task_uids: data.task_identity_aliases,
        stable_tasks: data.tasks.clone(),
        ..Queue::default()
    };
    for (uid, state) in data.tasks {
        if state.path_hint.is_empty() {
            continue;
        }
        if let Some(previous) = queue.task_uids.insert(state.path_hint.clone(), uid.clone()) {
            anyhow::ensure!(
                previous == uid,
                "Task queue state contains conflicting task path identities"
            );
        }
        queue.put_runtime_state(&state.path_hint, &state);
    }
    for (path, state) in data.legacy_tasks {
        queue.put_runtime_state(&path, &state);
    }
    Ok(queue)
}

pub(super) fn decode_value(value: serde_json::Value) -> anyhow::Result<(Queue, bool)> {
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
