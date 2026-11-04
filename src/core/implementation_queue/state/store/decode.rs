use super::{PersistedQueue, QUEUE_VERSION, from_current};

pub(in crate::core::implementation_queue::state) fn decode_value(
    value: serde_json::Value,
) -> anyhow::Result<super::Queue> {
    let version = value
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("Task queue state version must be an integer"))?;
    anyhow::ensure!(
        version == u64::from(QUEUE_VERSION),
        "Unsupported task queue state version {version}"
    );
    let data: PersistedQueue = serde_json::from_value(value)?;
    from_current(data)
}
