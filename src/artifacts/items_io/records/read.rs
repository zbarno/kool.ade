use super::{
    content::parse_content,
    model::{ItemRecord, LoadedItem, RECORD_SCHEMA_VERSION},
};
use crate::{
    artifacts::planning_store::{PlanningStore, StoreError, paths},
    domain::OpenItem,
};

pub fn load_store(store: &PlanningStore) -> anyhow::Result<(Vec<OpenItem>, Vec<OpenItem>, bool)> {
    store.with_consistent_read(|| load_store_unlocked(store))
}

pub(crate) fn load_store_unlocked(
    store: &PlanningStore,
) -> anyhow::Result<(Vec<OpenItem>, Vec<OpenItem>, bool)> {
    let records = read_records(store)?;
    if !records.is_empty() {
        let mut open = Vec::new();
        let mut resolved = Vec::new();
        for loaded in records {
            match loaded.item.status {
                crate::domain::ItemStatus::Open => open.push(loaded.item),
                crate::domain::ItemStatus::Resolved => resolved.push(loaded.item),
            }
        }
        validate_items(open.iter().chain(&resolved))?;
        return Ok((open, resolved, false));
    }
    let content_files = store.list_files(paths::ITEM_CONTENTS)?;
    anyhow::ensure!(
        content_files.is_empty(),
        "Item content files exist without matching state/items records"
    );
    let (open, open_exists) = read_open_items(store)?;
    let (resolved, resolved_exists) = read_resolved_items(store)?;
    validate_items(open.iter().chain(&resolved))?;
    Ok((open, resolved, open_exists || resolved_exists))
}

pub(super) fn read_records(store: &PlanningStore) -> anyhow::Result<Vec<LoadedItem>> {
    let mut records = Vec::new();
    for file in store.list_files(paths::ITEM_STATES)? {
        let uid = file.name.strip_suffix(".json").ok_or_else(|| {
            anyhow::anyhow!("Unexpected file in {}: {}", paths::ITEM_STATES, file.name)
        })?;
        let uid = uuid::Uuid::parse_str(uid)
            .map_err(|_| anyhow::anyhow!("Invalid item record filename {}", file.name))?
            .hyphenated()
            .to_string();
        let relative = format!("{}/{uid}.json", paths::ITEM_STATES);
        let bytes = store.read(&relative)?;
        let record: ItemRecord = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            record.schema_version == RECORD_SCHEMA_VERSION,
            "Unsupported item record schema version {} in {relative}",
            record.schema_version
        );
        anyhow::ensure!(record.uid == uid, "Item UID mismatch in {relative}");
        anyhow::ensure!(
            record.revision > 0,
            "Item record {relative} has revision zero"
        );
        anyhow::ensure!(
            record.created_at_ms <= record.updated_at_ms,
            "Item record {relative} has invalid timestamps"
        );
        let content_path = format!("{}/{uid}.md", paths::ITEM_CONTENTS);
        let content = String::from_utf8(store.read(&content_path)?)?;
        let content = parse_content(&content)?;
        anyhow::ensure!(
            content.uid == uid && content.id == record.data.id,
            "Item content identity does not match {relative}"
        );
        let loaded = LoadedItem::new(record, content);
        records.push(loaded);
    }
    records.sort_by(|left, right| {
        left.record
            .created_at_ms
            .cmp(&right.record.created_at_ms)
            .then_with(|| left.record.uid.cmp(&right.record.uid))
    });
    Ok(records)
}

fn read_open_items(store: &PlanningStore) -> anyhow::Result<(Vec<OpenItem>, bool)> {
    match store.read(paths::OPEN_ITEMS) {
        Ok(bytes) => {
            let text = String::from_utf8(bytes)?;
            let items = crate::artifacts::items_io::parse(&text).map_err(anyhow::Error::msg)?;
            Ok((items, true))
        }
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok((Vec::new(), false))
        }
        Err(error) => Err(error.into()),
    }
}

fn read_resolved_items(store: &PlanningStore) -> anyhow::Result<(Vec<OpenItem>, bool)> {
    match store.read(paths::RESOLVED_ITEMS) {
        Ok(bytes) => Ok((serde_json::from_slice(&bytes)?, true)),
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok((Vec::new(), false))
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_items<'a>(items: impl Iterator<Item = &'a OpenItem>) -> anyhow::Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    let mut uids = std::collections::BTreeSet::new();
    for item in items {
        anyhow::ensure!(
            crate::core::ids::is_valid_id(&item.id),
            "Invalid item ID {}",
            item.id
        );
        anyhow::ensure!(
            ids.insert(item.id.as_str()),
            "Duplicate item ID {}",
            item.id
        );
        if let Some(uid) = &item.uid {
            let canonical = uuid::Uuid::parse_str(uid)?.hyphenated().to_string();
            anyhow::ensure!(uids.insert(canonical), "Duplicate item UID {uid}");
        }
    }
    Ok(())
}
