use super::{Work, WorkRecord, normalize_task_intents, validate};
use crate::artifacts::planning_store::{PlanningRoot, RecordRevisionCheck, StoreError, paths};
use std::time::{SystemTime, UNIX_EPOCH};

const RECORD_SCHEMA_VERSION: u32 = 3;

mod discovered;
mod task_intent_events;
mod task_intent_history;
pub use discovered::{append_discovered, find};
pub(crate) use task_intent_events::linked_executable_task_changes;

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

pub fn load<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<Vec<Work>> {
    read_work(repo)
}

/// Load current work records against the caller's planning revision.
pub fn load_expected<R: PlanningRoot + ?Sized>(
    repo: &R,
    expected_revision: &str,
) -> anyhow::Result<(Vec<Work>, String)> {
    let store = repo.planning_store();
    let work = read_work(repo)?;
    let (_, revision) = store.transaction_with_revision(&[], Some(expected_revision))?;
    Ok((work, revision))
}

fn read_work<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<Vec<Work>> {
    let store = repo.planning_store();
    store.with_consistent_read(|| read_work_unlocked(&store))
}

fn read_work_unlocked(
    store: &crate::artifacts::planning_store::PlanningStore,
) -> anyhow::Result<Vec<Work>> {
    ensure_no_legacy_work_aggregate(store)?;
    let records = read_record_files(store)?;
    if !records.is_empty() {
        let work = records
            .into_iter()
            .map(|record| record.data)
            .collect::<Vec<_>>();
        validate(&work)?;
        return Ok(work);
    }
    Ok(Vec::new())
}

fn ensure_no_legacy_work_aggregate(
    store: &crate::artifacts::planning_store::PlanningStore,
) -> anyhow::Result<()> {
    match store.read(paths::WORK) {
        Ok(_) => anyhow::bail!(
            "Unsupported legacy planning work aggregate {}; current records were not changed",
            paths::WORK
        ),
        Err(StoreError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
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

/// Build atomic current-record writes and their revision fences.
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
    ensure_no_legacy_work_aggregate(store)?;
    let existing_records = read_record_files(store)?;
    let mut current = std::collections::BTreeMap::<String, WorkRecord>::new();
    for record in existing_records {
        current.insert(record.uid.clone(), record);
    }

    let mut desired = work.to_vec();

    for item in &mut desired {
        if item.parent_uid.is_some() || !item.kind.starts_task_intent() {
            continue;
        }
        if let Some(previous) = current.get(&item.uid) {
            if let Some(original_goal) = previous.data.original_goal.as_deref() {
                anyhow::ensure!(
                    item.original_goal.as_deref() == Some(original_goal),
                    "TaskIntent original goal is immutable"
                );
            } else {
                item.original_goal = Some(previous.data.request.clone());
            }
        } else {
            item.original_goal
                .get_or_insert_with(|| item.request.clone());
        }
    }
    let mut complete = current
        .values()
        .map(|record| record.data.clone())
        .collect::<Vec<_>>();
    for item in &desired {
        if let Some(existing) = complete
            .iter_mut()
            .find(|existing| existing.uid == item.uid)
        {
            *existing = item.clone();
        } else {
            complete.push(item.clone());
        }
    }
    normalize_task_intents(&mut complete);
    task_intent_history::append_events(&current, &mut desired, &mut complete)?;
    let normalized = complete
        .into_iter()
        .map(|item| (item.uid.clone(), item))
        .collect::<std::collections::BTreeMap<_, _>>();
    desired = desired
        .into_iter()
        .map(|item| normalized.get(&item.uid).cloned().unwrap_or(item))
        .collect();
    let mut complete = current
        .values()
        .map(|record| record.data.clone())
        .collect::<Vec<_>>();
    for item in &desired {
        if let Some(existing) = complete
            .iter_mut()
            .find(|existing| existing.uid == item.uid)
        {
            *existing = item.clone();
        } else {
            complete.push(item.clone());
        }
    }
    validate(&complete)?;

    let mut changes = Vec::new();
    let mut checks = Vec::new();
    let mut next_creation_time = now_ms();
    for item in desired {
        let uid = item.uid.clone();
        let previous = current.get(&uid);
        let baseline = item.record_baseline.as_deref();
        let current_data = serde_json::to_string(&item)?;
        if baseline == Some(current_data.as_str()) {
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
    Ok((changes, checks))
}
