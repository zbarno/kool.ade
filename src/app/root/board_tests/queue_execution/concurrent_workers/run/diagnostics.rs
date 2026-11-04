pub(super) fn execution_lock_diagnostics(root: &std::path::Path, task_uid: &str) -> String {
    let lock_state = match crate::persistence::execution_records::try_task_lock(task_uid) {
        Ok(Some(lock)) => {
            drop(lock);
            "available at probe time"
        }
        Ok(None) => "held (owner not identified at probe time)",
        Err(_) => "unavailable (probe error)",
    };
    let records = root.join("app-home").join("executions").join(task_uid);
    let mut records = std::fs::read_dir(records)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|entry| {
            let record = std::fs::read(entry.path()).ok().and_then(|bytes| {
                serde_json::from_slice::<crate::persistence::execution_records::ExecutionRecordV1>(
                    &bytes,
                )
                .ok()
            });
            match record {
                Some(record) => {
                    let updated_at = record.updated_at_unix_ms;
                    let execution_id = record.execution_id;
                    let summary = format!(
                        "{}:updated-{}/{:?}/{:?}/attempt-{}/command-started-{}",
                        execution_id,
                        updated_at,
                        record.state,
                        record.cleanup,
                        record.attempt,
                        record.command_started
                    );
                    (updated_at, execution_id, summary)
                }
                None => (0, String::new(), "unreadable-record".into()),
            }
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| (right.0, right.1.as_str()).cmp(&(left.0, left.1.as_str())));
    let total_records = records.len();
    let summaries = records
        .into_iter()
        .take(8)
        .map(|(_, _, summary)| summary)
        .collect::<Vec<_>>();
    format!(
        "lock={lock_state}, newest_records={summaries:?}, total_records={total_records}, omitted_records={}",
        total_records.saturating_sub(summaries.len())
    )
}
