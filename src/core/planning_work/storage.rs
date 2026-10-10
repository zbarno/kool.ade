use super::{SCHEMA_VERSION, Work, WorkFile, WorkRecord, validate};
use crate::artifacts::planning_store::{PlanningRoot, RecordRevisionCheck, StoreError, paths};
use serde::Deserialize;

const RECORD_SCHEMA_VERSION: u32 = 1;

mod discovered;
mod legacy;
pub use discovered::{append_discovered, find};
use legacy::{legacy_work_exists, migrate_legacy, now_ms};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWork {
    key: String,
    title: String,
    request: String,
    column: usize,
    feature: Option<String>,
    detail: String,
}

pub fn load<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<Vec<Work>> {
    read_work(repo).map(|(work, _)| work)
}

/// Load records against the caller's view. Legacy aggregates are converted to
/// independent record files on this explicit migration path; the originals
/// remain intact as recovery evidence.
pub fn load_expected<R: PlanningRoot + ?Sized>(
    repo: &R,
    expected_revision: &str,
) -> anyhow::Result<(Vec<Work>, String)> {
    let store = repo.planning_store();
    let (work, needs_migration) = read_work(repo)?;
    if needs_migration {
        let (changes, checks, _) = record_changes(&store, &work)?;
        let (_, revision) = store.transaction_with_revision_and_record_revisions(
            &changes,
            Some(expected_revision),
            &checks,
        )?;
        let (work, _) = read_work(&store)?;
        Ok((work, revision))
    } else {
        let (_, revision) = store.transaction_with_revision(&[], Some(expected_revision))?;
        Ok((work, revision))
    }
}

fn read_work<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<(Vec<Work>, bool)> {
    let store = repo.planning_store();
    store.with_consistent_read(|| read_work_unlocked(&store))
}

fn read_work_unlocked(
    store: &crate::artifacts::planning_store::PlanningStore,
) -> anyhow::Result<(Vec<Work>, bool)> {
    let records = read_record_files(store)?;
    if !records.is_empty() {
        let work = records
            .into_iter()
            .map(|record| record.data)
            .collect::<Vec<_>>();
        validate(&work)?;
        return Ok((work, false));
    }
    read_legacy_work(store)
}

fn read_record_files(
    store: &crate::artifacts::planning_store::PlanningStore,
) -> anyhow::Result<Vec<WorkRecord>> {
    let mut records = Vec::new();
    for file in store.list_files(paths::WORK_RECORDS)? {
        let uid = file.name.strip_suffix(".json").ok_or_else(|| {
            anyhow::anyhow!("Unexpected file in {}: {}", paths::WORK_RECORDS, file.name)
        })?;
        let uid = uuid::Uuid::parse_str(uid)
            .map_err(|_| anyhow::anyhow!("Invalid work record filename {}", file.name))?
            .hyphenated()
            .to_string();
        let relative = format!("{}/{uid}.json", paths::WORK_RECORDS);
        let bytes = store.read(&relative)?;
        let mut record: WorkRecord = serde_json::from_slice(&bytes)?;
        anyhow::ensure!(
            record.schema_version == RECORD_SCHEMA_VERSION,
            "Unsupported work record schema version {} in {relative}",
            record.schema_version
        );
        anyhow::ensure!(
            record.uid == uid && record.data.uid == uid,
            "Work UID mismatch in {relative}"
        );
        anyhow::ensure!(
            record.revision > 0,
            "Work record {relative} has revision zero"
        );
        anyhow::ensure!(
            record.created_at_ms <= record.updated_at_ms,
            "Work record {relative} has invalid timestamps"
        );
        record.data.record_revision = record.revision;
        record.data.record_baseline = Some(serde_json::to_string(&record.data)?);
        records.push(record);
    }
    records.sort_by(|left, right| {
        left.created_at_ms
            .cmp(&right.created_at_ms)
            .then_with(|| left.uid.cmp(&right.uid))
    });
    Ok(records)
}

fn read_legacy_work(
    store: &crate::artifacts::planning_store::PlanningStore,
) -> anyhow::Result<(Vec<Work>, bool)> {
    let bytes = match store.read(paths::WORK) {
        Ok(bytes) => bytes,
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok((Vec::new(), false));
        }
        Err(error) => return Err(error.into()),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    let work = if value.is_array() {
        serde_json::from_value::<Vec<LegacyWork>>(value)?
            .into_iter()
            .map(migrate_legacy)
            .collect::<anyhow::Result<Vec<_>>>()?
    } else {
        let file: WorkFile = serde_json::from_value(value)?;
        anyhow::ensure!(
            (1..=SCHEMA_VERSION).contains(&file.schema_version),
            "Unsupported planning work schema version {}",
            file.schema_version
        );
        file.items
    };
    validate(&work)?;
    Ok((work, true))
}

/// Build atomic record writes and their revision fences. Legacy aggregate
/// contents are included when the collection has not yet been migrated.
pub fn record_changes(
    store: &crate::artifacts::planning_store::PlanningStore,
    work: &[Work],
) -> anyhow::Result<crate::artifacts::planning_store::RecordChangePlan> {
    store.with_consistent_read(|| record_changes_unlocked(store, work))
}

fn record_changes_unlocked(
    store: &crate::artifacts::planning_store::PlanningStore,
    work: &[Work],
) -> anyhow::Result<crate::artifacts::planning_store::RecordChangePlan> {
    validate(work)?;
    let existing_records = read_record_files(store)?;
    let legacy_migration = existing_records.is_empty() && legacy_work_exists(store)?;
    let mut current = std::collections::BTreeMap::<String, WorkRecord>::new();
    let mut legacy_items = Vec::new();
    if !existing_records.is_empty() {
        for record in existing_records {
            current.insert(record.uid.clone(), record);
        }
    } else if legacy_migration {
        let (items, _) = read_legacy_work(store)?;
        legacy_items = items;
        for item in &legacy_items {
            current.insert(
                item.uid.clone(),
                WorkRecord {
                    schema_version: RECORD_SCHEMA_VERSION,
                    uid: item.uid.clone(),
                    revision: 0,
                    created_at_ms: 0,
                    updated_at_ms: 0,
                    data: item.clone(),
                },
            );
        }
    }

    let mut desired = work.to_vec();
    if legacy_migration {
        let included = desired
            .iter()
            .map(|item| item.uid.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for item in &legacy_items {
            if !included.contains(&item.uid) {
                desired.push(item.clone());
            }
        }
    }

    let mut changes = Vec::new();
    let mut checks = Vec::new();
    let mut next_creation_time = now_ms();
    for item in desired {
        let uid = item.uid.clone();
        let previous = current.get(&uid);
        let baseline = item.record_baseline.as_deref();
        let current_data = serde_json::to_string(&item)?;
        if !legacy_migration && baseline == Some(current_data.as_str()) {
            continue;
        }
        let expected_revision = item.record_revision;
        let path = format!("{}/{uid}.json", paths::WORK_RECORDS);
        let created_at_ms = match previous.filter(|record| record.created_at_ms > 0) {
            Some(record) => record.created_at_ms,
            None => {
                let created = next_creation_time;
                next_creation_time = next_creation_time.saturating_add(1);
                created
            }
        };
        let updated_at_ms = now_ms().max(created_at_ms);
        let record = WorkRecord {
            schema_version: RECORD_SCHEMA_VERSION,
            uid: uid.clone(),
            revision: expected_revision.saturating_add(1),
            created_at_ms,
            updated_at_ms,
            data: item,
        };
        changes.push((path.clone(), serde_json::to_vec_pretty(&record)?));
        checks.push(RecordRevisionCheck {
            path,
            expected_revision,
        });
    }
    Ok((changes, checks, legacy_migration))
}
