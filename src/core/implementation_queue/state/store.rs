use super::Queue;
use crate::core::implementation::Failure;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _, ser::Error as _};
use std::collections::{BTreeMap, BTreeSet};

mod decode;
pub(super) use decode::decode_value;

const QUEUE_VERSION: u32 = 6;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct TaskQueueState {
    pub(super) current: bool,
    pub(super) in_flight: bool,
    pub(super) blocked: Option<Failure>,
    pub(super) recovery_attempts: usize,
    pub(super) waiting_since_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PersistedQueue {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    auto_plan: bool,
    auto_build: bool,
    auto_publish: bool,
    require_independent_checks: bool,
    running: bool,
    max_parallel: usize,
    last_error: String,
    recovery_paused: bool,
    pub(super) tasks: BTreeMap<String, TaskQueueState>,
}

impl Serialize for Queue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        persisted(self)
            .map_err(S::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Queue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        decode_value(value).map_err(D::Error::custom)
    }
}

pub(super) fn persisted(queue: &Queue) -> anyhow::Result<PersistedQueue> {
    let mut tasks = queue
        .stable_tasks
        .iter()
        .filter(|(_, state)| has_state(state))
        .map(|(uid, state)| (uid.clone(), state.clone()))
        .collect::<BTreeMap<_, _>>();
    for uid in queue.current_paths.keys() {
        tasks.remove(uid);
    }
    let mut paths = queue.in_flight.iter().cloned().collect::<BTreeSet<_>>();
    paths.extend(queue.blocked.keys().cloned());
    paths.extend(queue.recovery_attempts.keys().cloned());
    paths.extend(queue.current_ticket.iter().cloned());
    paths.extend(queue.waiting_since_unix_ms.keys().cloned());
    for path in paths {
        let uid = queue.task_uids.get(&path).ok_or_else(|| {
            anyhow::anyhow!("Queued task state has no current stable task identity: {path}")
        })?;
        let state = TaskQueueState {
            current: queue.current_ticket.as_deref() == Some(path.as_str()),
            in_flight: queue.in_flight.contains(&path),
            blocked: queue.blocked.get(&path).cloned(),
            recovery_attempts: queue
                .recovery_attempts
                .get(&path)
                .copied()
                .unwrap_or_default(),
            waiting_since_unix_ms: queue.waiting_since_unix_ms.get(&path).copied(),
        };
        if has_state(&state) {
            tasks.insert(uid.clone(), state);
        }
    }
    Ok(PersistedQueue {
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
    })
}

fn has_state(state: &TaskQueueState) -> bool {
    state.current
        || state.in_flight
        || state.blocked.is_some()
        || state.recovery_attempts > 0
        || state.waiting_since_unix_ms.is_some()
}

fn from_current(data: PersistedQueue) -> anyhow::Result<Queue> {
    anyhow::ensure!(
        data.schema_version == QUEUE_VERSION,
        "Task queue schema version is inconsistent"
    );
    anyhow::ensure!(
        !data.auto_publish || data.require_independent_checks,
        "Task queue publication policy is missing its independent-check requirement"
    );
    anyhow::ensure!(
        (1..=8).contains(&data.max_parallel),
        "Task queue maximum parallelism is invalid"
    );
    let mut queue = Queue {
        auto_plan: data.auto_plan,
        auto_build: data.auto_build,
        auto_publish: data.auto_publish,
        require_independent_checks: data.require_independent_checks,
        running: data.running,
        max_parallel: data.max_parallel,
        last_error: data.last_error,
        recovery_paused: data.recovery_paused,
        stable_tasks: BTreeMap::new(),
        ..Queue::default()
    };
    for (uid, state) in data.tasks {
        let parsed = uuid::Uuid::parse_str(&uid)
            .map_err(|_| anyhow::anyhow!("Task queue contains an invalid task UID"))?;
        anyhow::ensure!(
            parsed.hyphenated().to_string() == uid,
            "Task queue contains a noncanonical task UID"
        );
        queue.stable_tasks.insert(uid, state);
    }
    Ok(queue)
}
