use std::path::Path;

use super::super::{
    git,
    journal::{OperationJournal, PlanningSyncResolution, RecordMutation, SyncOperation},
    merge,
    model::{PlanningConflictChoice, PlanningSyncStatus},
};
use super::{
    intent::{journal_lock, now_ms, read_worktree_file, shared_manifest},
    refresh::{FetchError, fetch_verified},
};
mod rebase;

pub(crate) fn resolve_conflict_at(
    repo: &Path,
    state_root: &Path,
    source_operation_id: uuid::Uuid,
    path: &str,
    choice: PlanningConflictChoice,
    manual_value: Option<&str>,
) -> anyhow::Result<PlanningSyncStatus> {
    let (manifest, remote_configured) = shared_manifest(repo)?
        .ok_or_else(|| anyhow::anyhow!("planning repository is not managed"))?;
    anyhow::ensure!(
        remote_configured,
        "planning repository has no shared remote"
    );
    let canonical_branch = manifest
        .canonical_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&manifest.canonical_ref);
    anyhow::ensure!(
        git::is_canonical_branch(repo, &manifest.canonical_ref)?,
        "planning checkout must be on canonical branch {canonical_branch} before resolving conflicts"
    );
    let _repository_lock = crate::core::gitops::lock_repository(repo)?;
    let lock = journal_lock(state_root, manifest.workspace_id)?;
    let mut journal =
        OperationJournal::load_or_new(state_root, manifest.workspace_id, &manifest.canonical_ref)?;
    let source_index = journal
        .operations
        .iter()
        .position(|operation| operation.operation_id == source_operation_id)
        .ok_or_else(|| anyhow::anyhow!("pending operation no longer exists"))?;
    let conflict = journal
        .conflicts
        .iter()
        .find(|conflict| conflict.operation_id == source_operation_id && conflict.path == path)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("planning conflict is no longer pending"))?;
    let expected_local = journal
        .operations
        .iter()
        .rev()
        .find_map(|operation| {
            operation
                .mutations
                .iter()
                .find(|mutation| mutation.path == path)
        })
        .map(|mutation| mutation.desired.as_deref().map(str::as_bytes))
        .unwrap_or_else(|| conflict.local_value.as_deref().map(str::as_bytes));
    let local_value = read_worktree_file(repo, path)?;
    anyhow::ensure!(
        local_value.as_deref() == expected_local,
        "the local record changed after this conflict was displayed; refresh and review it again"
    );
    let remote_commit = match fetch_verified(repo, &manifest) {
        Ok(commit) => commit,
        Err(FetchError::Transport(error)) => return Err(error),
        Err(FetchError::Incompatible) => {
            anyhow::bail!("planning remote has an incompatible schema")
        }
    };
    let remote_value = git::read_path(repo, &remote_commit, path)?;
    let remote_text = remote_value
        .as_deref()
        .map(|bytes| String::from_utf8(bytes.to_vec()))
        .transpose()?;
    anyhow::ensure!(
        remote_text == conflict.remote_value,
        "the remote record changed after this conflict was displayed; refresh and review it again"
    );
    let original = journal.operations[source_index]
        .mutations
        .iter()
        .find(|mutation| mutation.path == path)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("pending operation no longer contains this record"))?;
    let source_local_commit = journal.operations[source_index].local_commit.clone();

    let desired = match choice {
        PlanningConflictChoice::Remote => remote_text.clone(),
        PlanningConflictChoice::Local => original.desired.clone(),
        PlanningConflictChoice::Manual => Some(
            manual_value
                .ok_or_else(|| anyhow::anyhow!("manual resolution text is required"))?
                .to_owned(),
        ),
    };
    let resolution_id = uuid::Uuid::new_v4();
    let resolution_at_ms = now_ms();
    let mut mutations = Vec::new();
    let mut selected_value = desired.clone();
    if choice != PlanningConflictChoice::Remote {
        let mutation =
            resolved_mutation(&original, path, remote_text.as_deref(), desired.as_deref())?;
        merge::validate_resolution(&mutation)?;
        selected_value = mutation.desired.clone();
        mutations.push(mutation);
    }
    mutations.push(resolution_record(ResolutionRecord {
        resolution_id,
        workspace_id: manifest.workspace_id,
        source_operation_id,
        record_id: &conflict.record_id,
        path,
        choice,
        remote_commit: &remote_commit,
        created_at_ms: resolution_at_ms,
    })?);
    let new_operation_id = uuid::Uuid::new_v4();
    let resolution_operation = SyncOperation {
        operation_id: new_operation_id,
        workspace_id: manifest.workspace_id,
        base_commit: remote_commit.clone(),
        canonical_ref: manifest.canonical_ref.clone(),
        message: format!("Resolve planning conflict in {}", conflict.record_id),
        created_at_ms: resolution_at_ms,
        remote_base_commit: None,
        local_commit: source_local_commit,
        mutations,
    };

    for operation in &mut journal.operations {
        if operation.operation_id == source_operation_id {
            operation.mutations.retain(|mutation| mutation.path != path);
        }
    }
    journal
        .operations
        .retain(|operation| !operation.mutations.is_empty());
    let insertion_index = if journal
        .operations
        .get(source_index)
        .is_some_and(|operation| operation.operation_id == source_operation_id)
    {
        source_index + 1
    } else {
        source_index
    };
    journal
        .operations
        .insert(insertion_index, resolution_operation);
    if choice != PlanningConflictChoice::Remote {
        rebase::later_operations(
            &mut journal.operations[insertion_index + 1..],
            path,
            original.desired.as_deref(),
            selected_value.as_deref(),
        )?;
    }
    journal
        .conflicts
        .retain(|pending| pending.operation_id != source_operation_id || pending.path != path);
    journal.resolutions.push(PlanningSyncResolution {
        resolution_id,
        source_operation_id,
        path: path.to_owned(),
        choice,
        remote_commit: remote_commit.clone(),
        new_operation_id: Some(new_operation_id),
        created_at_ms: resolution_at_ms,
    });
    let latest_value = journal
        .operations
        .iter()
        .rev()
        .find_map(|operation| {
            operation
                .mutations
                .iter()
                .find(|mutation| mutation.path == path)
        })
        .map(|mutation| mutation.desired.as_deref())
        .unwrap_or(selected_value.as_deref());
    git::write_worktree_path(repo, path, latest_value.map(str::as_bytes))?;
    journal.confirmed_commit = Some(remote_commit);
    journal.confirmed_at_ms = Some(now_ms());
    journal.state = if journal.operations.is_empty() {
        super::super::model::PlanningSyncState::Synced
    } else {
        super::super::model::PlanningSyncState::Pending
    };
    journal.detail = None;
    journal.write(state_root)?;
    drop(lock);
    super::sync_with_state_root_locked(repo, state_root)
}

struct ResolutionRecord<'a> {
    resolution_id: uuid::Uuid,
    workspace_id: uuid::Uuid,
    source_operation_id: uuid::Uuid,
    record_id: &'a str,
    path: &'a str,
    choice: PlanningConflictChoice,
    remote_commit: &'a str,
    created_at_ms: u64,
}

fn resolution_record(input: ResolutionRecord<'_>) -> anyhow::Result<RecordMutation> {
    let uid = input.resolution_id.hyphenated().to_string();
    let record_path = format!("state/planning_sync/resolutions/{uid}.json");
    let record = serde_json::json!({
        "schemaVersion": 1,
        "uid": uid,
        "revision": 1,
        "createdAtMs": input.created_at_ms,
        "updatedAtMs": input.created_at_ms,
        "data": {
            "workspaceId": input.workspace_id.hyphenated().to_string(),
            "sourceOperationId": input.source_operation_id.hyphenated().to_string(),
            "recordId": input.record_id,
            "path": input.path,
            "choice": input.choice,
            "remoteCommit": input.remote_commit
        }
    });
    let mutation = RecordMutation {
        path: record_path,
        record_id: uid,
        expected_revision: Some(0),
        base: None,
        desired: Some(serde_json::to_string_pretty(&record)?),
    };
    merge::validate_resolution(&mutation)?;
    Ok(mutation)
}

fn resolved_mutation(
    original: &RecordMutation,
    path: &str,
    remote: Option<&str>,
    desired: Option<&str>,
) -> anyhow::Result<RecordMutation> {
    let mut expected_revision = None;
    let mut desired = desired.map(str::to_owned);
    if let Some((uid, _)) = merge::record_json_uid(path) {
        let base_revision = remote
            .map(serde_json::from_str::<serde_json::Value>)
            .transpose()?
            .map(|value| {
                value
                    .get("revision")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| anyhow::anyhow!("remote record revision is missing"))
            })
            .transpose()?
            .unwrap_or(0);
        expected_revision = Some(base_revision);
        if let Some(value) = desired.as_mut() {
            let mut record: serde_json::Value = serde_json::from_str(value)?;
            anyhow::ensure!(
                record.get("uid").and_then(serde_json::Value::as_str) == Some(uid),
                "resolved record identity does not match its path"
            );
            let object = record
                .as_object_mut()
                .ok_or_else(|| anyhow::anyhow!("resolved record must be a JSON object"))?;
            object.insert(
                "revision".into(),
                serde_json::Value::from(base_revision + 1),
            );
            object.insert("updatedAtMs".into(), serde_json::Value::from(now_ms()));
            *value = serde_json::to_string_pretty(&record)?;
        }
    }
    Ok(RecordMutation {
        path: path.to_owned(),
        record_id: original.record_id.clone(),
        expected_revision,
        base: remote.map(str::to_owned),
        desired,
    })
}
