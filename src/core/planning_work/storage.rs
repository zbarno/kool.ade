use super::{SCHEMA_VERSION, Work, WorkFile, WorkKind, WorkStatus, save_expected, validate};
use crate::artifacts::planning_store::PlanningRoot;
use serde::Deserialize;

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

/// Load planning work against the caller's revision and upgrade old schemas
/// transactionally when needed. Returns the revision associated with the
/// loaded work, including the post-upgrade revision.
pub fn load_expected<R: PlanningRoot + ?Sized>(
    repo: &R,
    expected_revision: &str,
) -> anyhow::Result<(Vec<Work>, String)> {
    let store = repo.planning_store();
    let (work, needs_upgrade) = read_work(repo)?;
    let (_, read_revision) = store.transaction_with_revision(&[], Some(expected_revision))?;
    let revision = if needs_upgrade {
        save_expected(repo, &work, expected_revision)?
    } else {
        read_revision
    };
    Ok((work, revision))
}

fn read_work<R: PlanningRoot + ?Sized>(repo: &R) -> anyhow::Result<(Vec<Work>, bool)> {
    let store = repo.planning_store();
    let bytes = match store.read(super::STORE_FILE) {
        Ok(bytes) => bytes,
        Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            return Ok((Vec::new(), false));
        }
        Err(error) => return Err(error.into()),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if value.is_array() {
        let legacy: Vec<LegacyWork> = serde_json::from_value(value)?;
        let work: Vec<Work> = legacy
            .into_iter()
            .map(migrate_legacy)
            .collect::<anyhow::Result<_>>()?;
        return Ok((work, true));
    }
    let file: WorkFile = serde_json::from_value(value)?;
    anyhow::ensure!(
        (1..=SCHEMA_VERSION).contains(&file.schema_version),
        "Unsupported planning work schema version {}",
        file.schema_version
    );
    validate(&file.items)?;
    Ok((file.items, file.schema_version < SCHEMA_VERSION))
}

fn migrate_legacy(work: LegacyWork) -> anyhow::Result<Work> {
    Ok(Work {
        uid: legacy_uid(&work.key),
        key: work.key,
        kind: WorkKind::Feature,
        title: work.title,
        request: work.request,
        status: WorkStatus::from_legacy_column(work.column)?,
        feature_id: work.feature,
        feature_uid: None,
        parent_uid: None,
        source_branch: None,
        destination_branch: None,
        routing_overrides: std::collections::BTreeMap::new(),
        routing_inherited_from: None,
        follow_up_task: None,
        detail: work.detail,
    })
}

fn legacy_uid(key: &str) -> String {
    use sha2::{Digest, Sha256};

    let mut bytes: [u8; 16] = Sha256::digest(key.as_bytes())[..16]
        .try_into()
        .expect("SHA-256 provides at least 16 bytes");
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).hyphenated().to_string()
}

pub fn append_discovered<R: PlanningRoot + ?Sized>(
    repo: &R,
    drafts: &[crate::harness::PlanningTaskDraft],
) -> anyhow::Result<Option<String>> {
    if drafts.is_empty() {
        return Ok(None);
    }
    let mut items = load(repo)?;
    for draft in drafts {
        let mut work = Work::new(
            String::new(),
            draft.title.trim().to_owned(),
            draft.description.trim().to_owned(),
            "Discovered during repository documentation review; triage before acting.".into(),
        );
        work.key = format!("task:{}", work.uid);
        work.kind = draft.kind;
        work.status = draft.status;
        items.push(work);
    }
    validate(&items)?;
    Ok(Some(serde_json::to_string_pretty(&WorkFile {
        schema_version: SCHEMA_VERSION,
        items,
    })?))
}

pub fn find(state: &crate::core::state::PlannerState, key: &str) -> Option<Work> {
    load(&state.planning_store)
        .ok()?
        .into_iter()
        .find(|item| item.key == key)
}
