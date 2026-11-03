use crate::{
    artifacts::planning_store::{PlanningStore, StoreError, paths},
    core::planning_work::Work,
    domain::OpenItem,
};

/// Check persisted decision links against the authoritative planning records.
/// Read raw records here because callers already hold the planning-store lock.
pub(super) fn validate_links(
    store: &PlanningStore,
    items: impl Iterator<Item = OpenItem>,
) -> anyhow::Result<()> {
    let linked = items
        .filter_map(|item| {
            item.task_intent_uid
                .map(|uid| (uid, item.task_intent_cycle_uid))
        })
        .collect::<Vec<_>>();
    if linked.is_empty() {
        return Ok(());
    }
    for (uid, _) in &linked {
        anyhow::ensure!(
            uuid::Uuid::parse_str(uid).is_ok(),
            "Planning item has an invalid TaskIntent UID"
        );
    }
    let roots = read_work_records(store)?;
    for (uid, cycle_uid) in linked {
        let matches = roots
            .iter()
            .filter(|work| {
                work.uid == uid
                    && work.parent_uid.is_none()
                    && work.kind.starts_task_intent()
                    && work.task_intent_root_uid() == Some(uid.as_str())
            })
            .collect::<Vec<_>>();
        anyhow::ensure!(
            matches.len() == 1,
            "Planning item TaskIntent UID must identify exactly one valid root: {uid}"
        );
        if let Some(cycle_uid) = cycle_uid {
            anyhow::ensure!(
                matches[0]
                    .task_intent_cycles
                    .iter()
                    .any(|cycle| cycle.uid == cycle_uid),
                "Planning item references an unknown TaskIntent cycle: {cycle_uid}"
            );
        }
    }
    Ok(())
}

fn read_work_records(store: &PlanningStore) -> anyhow::Result<Vec<Work>> {
    let records = store.list_files(paths::WORK_RECORDS)?;
    if !records.is_empty() {
        return records
            .into_iter()
            .map(|file| {
                let filename_uid = file.name.strip_suffix(".json").ok_or_else(|| {
                    anyhow::anyhow!("Unexpected file in {}: {}", paths::WORK_RECORDS, file.name)
                })?;
                let canonical_filename_uid = uuid::Uuid::parse_str(filename_uid)?
                    .hyphenated()
                    .to_string();
                anyhow::ensure!(
                    canonical_filename_uid == filename_uid,
                    "Work record filename is not canonical"
                );
                let value: serde_json::Value = serde_json::from_slice(&store.read(format!(
                    "{}/{}",
                    paths::WORK_RECORDS,
                    file.name
                ))?)?;
                let schema_version = value
                    .get("schemaVersion")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| anyhow::anyhow!("Work record has no schema version"))?;
                anyhow::ensure!(
                    (1..=3).contains(&schema_version),
                    "Unsupported work record schema version {schema_version}"
                );
                let record_uid = value
                    .get("uid")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("Work record has no UID"))?;
                anyhow::ensure!(
                    record_uid == filename_uid,
                    "Work record filename does not match its UID"
                );
                let revision = value
                    .get("revision")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                let created = value
                    .get("createdAtMs")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(u64::MAX);
                let updated = value
                    .get("updatedAtMs")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0);
                anyhow::ensure!(
                    revision > 0 && created <= updated,
                    "Work record revision or timestamps are invalid"
                );
                let work: Work = serde_json::from_value(
                    value
                        .get("data")
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("Work record has no data"))?,
                )?;
                anyhow::ensure!(
                    work.uid == record_uid,
                    "Work record UID does not match its data"
                );
                Ok(work)
            })
            .collect();
    }

    let bytes = match store.read(paths::WORK) {
        Ok(bytes) => bytes,
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("Planning item TaskIntent root does not exist")
        }
        Err(error) => return Err(error.into()),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let items = value.get("items").ok_or_else(|| {
        anyhow::anyhow!("Legacy planning work cannot own TaskIntent-linked items")
    })?;
    Ok(serde_json::from_value(items.clone())?)
}
