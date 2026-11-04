use super::{
    content::serialize_content,
    model::{ItemRecord, ItemState, RECORD_SCHEMA_VERSION},
    read::read_records,
};
use crate::artifacts::planning_store::{
    PlanningStore, RecordChangePlan, RecordRevisionCheck, paths,
};
use crate::domain::{ItemStatus, OpenItem};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn record_changes(
    store: &PlanningStore,
    open_items: &[OpenItem],
    resolved_items: &[OpenItem],
) -> anyhow::Result<RecordChangePlan> {
    store.with_consistent_read(|| record_changes_unlocked(store, open_items, resolved_items))
}

fn record_changes_unlocked(
    store: &PlanningStore,
    open_items: &[OpenItem],
    resolved_items: &[OpenItem],
) -> anyhow::Result<RecordChangePlan> {
    validate_items(open_items.iter().chain(resolved_items))?;
    super::task_intent::validate_links(store, open_items.iter().chain(resolved_items).cloned())?;
    let records = read_records(store)?;
    let current = records
        .into_iter()
        .map(|loaded| (loaded.record.uid.clone(), loaded))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut desired = Vec::with_capacity(open_items.len() + resolved_items.len());
    desired.extend(open_items.iter().cloned().map(|mut item| {
        item.status = ItemStatus::Open;
        item
    }));
    desired.extend(resolved_items.iter().cloned().map(|mut item| {
        item.status = ItemStatus::Resolved;
        item
    }));
    normalize_uids(&mut desired)?;

    let mut changes = Vec::new();
    let mut checks = Vec::new();
    let mut next_creation_time = now_ms();
    for item in desired {
        let uid = item.uid.clone().expect("normalized item UID");
        let previous = current.get(&uid);
        let baseline = item.record_baseline.as_deref();
        let current = serde_json::to_string(&item)?;
        if baseline == Some(current.as_str()) {
            continue;
        }
        let expected_revision = item.record_revision;
        let state_path = format!("{}/{uid}.json", paths::ITEM_STATES);
        let content_path = format!("{}/{uid}.md", paths::ITEM_CONTENTS);
        let created_at_ms = match previous.filter(|loaded| loaded.record.created_at_ms > 0) {
            Some(loaded) => loaded.record.created_at_ms,
            None => {
                let created = next_creation_time;
                next_creation_time = next_creation_time.saturating_add(1);
                created
            }
        };
        let updated_at_ms = now_ms().max(created_at_ms);
        let record = ItemRecord {
            schema_version: RECORD_SCHEMA_VERSION,
            uid,
            revision: expected_revision.saturating_add(1),
            created_at_ms,
            updated_at_ms,
            data: ItemState::from(&item),
        };
        changes.push((state_path.clone(), serde_json::to_vec_pretty(&record)?));
        changes.push((content_path, serialize_content(&item)?.into_bytes()));
        checks.push(RecordRevisionCheck {
            path: state_path,
            expected_revision,
        });
    }
    Ok((changes, checks))
}

fn validate_items<'a>(items: impl Iterator<Item = &'a OpenItem>) -> anyhow::Result<()> {
    let mut ids = std::collections::BTreeSet::new();
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
    }
    Ok(())
}

fn normalize_uids(items: &mut [OpenItem]) -> anyhow::Result<()> {
    for item in items {
        let uid = item
            .uid
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Item {} has no immutable UID", item.id))?;
        let uid = uuid::Uuid::parse_str(uid)?.hyphenated().to_string();
        item.uid = Some(uid);
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}
