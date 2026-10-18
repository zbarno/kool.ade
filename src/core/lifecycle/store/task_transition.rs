use serde::{Deserialize, Serialize};

use crate::artifacts::planning_store::{PlanningStore, StoreError};
use crate::core::lifecycle::TaskPhase;

use super::{
    FeatureGoal, LifecycleEventKind, Snapshot, TaskEvent, TaskRecord,
    records::{
        append_transition_with_additional_checks, encode_record, event_path, event_record,
        feature_path, now_unix_ms, pr_work_item_path, stale, task_actor, task_path,
    },
};

mod completion;
mod load;
pub(super) use completion::ensure_feature_tasks_complete;
pub(super) use load::load;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestPreparationStatus {
    AwaitingAuthorization,
    Publishing,
    NeedsAttention,
    Opened,
    ClosedUnmerged,
    Merged,
}

/// Durable, idempotently identified follow-up work created with task Done.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PullRequestPreparation {
    pub schema_version: u16,
    /// Canonical planning-store record identity.
    pub uid: String,
    pub work_item_uid: String,
    pub feature_uid: String,
    pub task_uid: String,
    /// The parent task and its directly owned subtasks are scoped into this PR.
    pub included_task_uids: Vec<String>,
    pub revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub status: PullRequestPreparationStatus,
    /// Publication authority must be granted through a separate app-side action.
    pub publication_authorization: Option<super::super::GateReceipt>,
    #[serde(default)]
    pub provider_pr_id: Option<u64>,
    #[serde(default)]
    pub provider_pr_url: Option<String>,
    #[serde(default)]
    pub head_sha: Option<String>,
    #[serde(default)]
    pub base_sha: Option<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub merged_at: Option<String>,
    /// Merge commit SHA reported by the provider; it does not prove local checkout state.
    #[serde(default)]
    pub merged_commit_sha: Option<String>,
}

/// Provider state observed after a pull request publication or refresh.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PullRequestPublicationState {
    pub status: PullRequestPreparationStatus,
    pub provider_pr_id: Option<u64>,
    pub provider_pr_url: Option<String>,
    pub head_sha: Option<String>,
    pub base_sha: Option<String>,
    pub base_branch: Option<String>,
    pub merged_at: Option<String>,
    pub merged_commit_sha: Option<String>,
}

impl PullRequestPreparation {
    pub(super) fn validate(&self) -> Result<(), StoreError> {
        let valid_uid = |value: &str| uuid::Uuid::parse_str(value).is_ok();
        if self.schema_version != 1
            || !valid_uid(&self.uid)
            || !valid_uid(&self.work_item_uid)
            || self.uid != self.work_item_uid
            || !valid_uid(&self.feature_uid)
            || !valid_uid(&self.task_uid)
            || self.included_task_uids.is_empty()
            || self.included_task_uids.len() > 256
            || self.included_task_uids.iter().any(|uid| !valid_uid(uid))
            || !self.included_task_uids.contains(&self.task_uid)
            || self
                .included_task_uids
                .iter()
                .enumerate()
                .any(|(index, uid)| {
                    self.included_task_uids[..index]
                        .iter()
                        .any(|prior| prior == uid)
                })
            || self.revision == 0
            || self.created_at_ms == 0
            || self.updated_at_ms < self.created_at_ms
            || self.provider_pr_id == Some(0)
            || self
                .provider_pr_url
                .as_deref()
                .is_some_and(|url| !url.starts_with("https://"))
            || self
                .head_sha
                .as_deref()
                .is_some_and(|sha| !super::super::types::is_git_oid(sha))
            || self
                .base_sha
                .as_deref()
                .is_some_and(|sha| !super::super::types::is_git_oid(sha))
            || self
                .merged_commit_sha
                .as_deref()
                .is_some_and(|sha| !super::super::types::is_git_oid(sha))
            || self
                .publication_authorization
                .as_ref()
                .is_some_and(|receipt| {
                    receipt.validate().is_err()
                        || receipt.actor != super::super::GateActor::User
                        || receipt.decision != super::super::GateDecision::Approved
                        || receipt.subject_uid != self.work_item_uid
                })
            || (self.status != PullRequestPreparationStatus::AwaitingAuthorization
                && self.publication_authorization.is_none())
            || (matches!(
                self.status,
                PullRequestPreparationStatus::Opened
                    | PullRequestPreparationStatus::ClosedUnmerged
                    | PullRequestPreparationStatus::Merged
            ) && (self.provider_pr_url.is_none() || self.head_sha.is_none()))
        {
            return Err(StoreError::MalformedState(
                "pull request preparation record is invalid".into(),
            ));
        }
        Ok(())
    }
}

pub(super) fn apply_task_event(
    store: &PlanningStore,
    task_uid: &str,
    expected_revision: u64,
    event_uid: &str,
    proposal: TaskEvent,
) -> Result<TaskRecord, StoreError> {
    let path = task_path(task_uid);
    let (mut snapshot, actual) = store.read_record::<Snapshot<TaskRecord>>(&path)?;
    if actual != expected_revision || snapshot.record.revision != expected_revision {
        return Err(stale(&path, expected_revision, actual));
    }
    if matches!(
        &proposal,
        TaskEvent::ReconcileSpec { .. } | TaskEvent::SubtaskReconciled { .. }
    ) {
        let lifecycle = super::LifecycleStore::new(store);
        lifecycle.ensure_task_specification_current(&snapshot.record)?;
    }
    snapshot
        .record
        .apply(expected_revision, proposal.clone())
        .map_err(StoreError::MalformedState)?;
    snapshot.revision = snapshot.record.revision;
    snapshot.updated_at_ms = now_unix_ms();
    let event_path = event_path(event_uid)?;
    let mut event = event_record(
        event_uid,
        &snapshot.record.feature_uid,
        Some(task_uid),
        expected_revision,
        task_actor(&proposal),
        LifecycleEventKind::Task(proposal.clone()),
        super::records::EventProjection::default(),
    )?;
    if let TaskEvent::ReconcileSpec {
        pr_work_item_uid, ..
    } = proposal
    {
        let feature_record_path = feature_path(&snapshot.record.feature_uid);
        let (feature_snapshot, feature_revision) =
            store.read_record::<Snapshot<FeatureGoal>>(&feature_record_path)?;
        if feature_snapshot.record.feature_uid != snapshot.record.feature_uid
            || feature_snapshot.uid != snapshot.record.feature_uid
            || feature_snapshot.revision != feature_revision
            || !feature_snapshot
                .record
                .task_uids
                .contains(&task_uid.to_owned())
        {
            return Err(StoreError::MalformedState(
                "PR preparation requires the current task in its approved feature graph".into(),
            ));
        }
        let mut included_task_uids = vec![task_uid.to_owned()];
        let mut discovered_subtasks = Vec::new();
        for child_uid in &feature_snapshot.record.task_uids {
            if child_uid == task_uid {
                continue;
            }
            let child_path = task_path(child_uid);
            let (child_snapshot, child_revision) =
                store.read_record::<Snapshot<TaskRecord>>(&child_path)?;
            let child = child_snapshot.record;
            if child_snapshot.uid != *child_uid
                || child_revision != child_snapshot.revision
                || child.revision != child_revision
                || child.feature_uid != snapshot.record.feature_uid
            {
                return Err(StoreError::MalformedState(
                    "feature task graph contains an inconsistent subtask record".into(),
                ));
            }
            if child.parent_task_uid.as_deref() == Some(task_uid) {
                if !snapshot.record.subtask_uids.contains(child_uid) {
                    return Err(StoreError::MalformedState(
                        "subtask ownership is missing from its parent record".into(),
                    ));
                }
                if child.phase != TaskPhase::Done || child.pr_work_item_uid.is_some() {
                    return Err(StoreError::MalformedState(
                        "parent task cannot complete before all subtasks are done".into(),
                    ));
                }
                child.validate().map_err(StoreError::MalformedState)?;
                included_task_uids.push(child_uid.clone());
                discovered_subtasks.push(child_uid.clone());
            }
        }
        if discovered_subtasks.len() != snapshot.record.subtask_uids.len()
            || snapshot
                .record
                .subtask_uids
                .iter()
                .any(|uid| !discovered_subtasks.contains(uid))
        {
            return Err(StoreError::MalformedState(
                "parent task graph references missing or misowned subtasks".into(),
            ));
        }
        let now = now_unix_ms();
        let preparation = PullRequestPreparation {
            schema_version: 1,
            uid: pr_work_item_uid.clone(),
            work_item_uid: pr_work_item_uid.clone(),
            feature_uid: snapshot.record.feature_uid.clone(),
            task_uid: task_uid.to_owned(),
            included_task_uids,
            revision: 1,
            created_at_ms: now,
            updated_at_ms: now,
            status: PullRequestPreparationStatus::AwaitingAuthorization,
            publication_authorization: None,
            provider_pr_id: None,
            provider_pr_url: None,
            head_sha: None,
            base_sha: None,
            base_branch: None,
            merged_at: None,
            merged_commit_sha: None,
        };
        preparation.validate()?;
        let preparation_snapshot = Snapshot {
            uid: pr_work_item_uid.clone(),
            schema_version: 1,
            created_at_ms: now,
            updated_at_ms: now,
            revision: 1,
            record: preparation,
        };
        event.pr_preparation_after = Some(preparation_snapshot.record.clone());
        let mut additional = super::spec_sync_queue::complete_for_task(
            store,
            &snapshot.record.feature_uid,
            task_uid,
        )?;
        additional.changes.push((
            pr_work_item_path(&pr_work_item_uid)?,
            encode_record(&preparation_snapshot)?,
        ));
        append_transition_with_additional_checks(
            store,
            &path,
            &snapshot,
            expected_revision,
            event_path,
            event,
            additional,
        )?;
    } else {
        super::records::append_transition(
            store,
            &path,
            &snapshot,
            expected_revision,
            event_path,
            event,
        )?;
    }
    Ok(snapshot.record)
}
