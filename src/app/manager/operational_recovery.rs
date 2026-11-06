//! Native operational triage remains available when Docker/provider execution fails.
use crate::{
    app::session::Project,
    core::planning_work::{
        Work, WorkStatus,
        operational_recovery::{self, FailureKind},
    },
    domain::ChatRole,
};

mod dispatch;
mod investigation;
pub(crate) use dispatch::prepare_reply_dispatch;
const ACTIVE_REPLY_DETAIL: &str = "Manager is monitoring the existing saved-answer recovery. Completion requires a durable application receipt.";
mod native;
pub(crate) use investigation::{
    blocked as investigation_blocked, callback_state as investigation_callback_state,
    claim as claim_investigation, enqueue as enqueue_investigation,
};
mod source;
pub(crate) use native::NativeWorker;
use source::source;

pub(crate) fn advance(project: &mut Project) {
    if native::poll(project) {
        return;
    }
    project.bind_task_conversation_identities();
    project.task_chats.ensure_loaded(&project.chat_slug);
    let original = project.planning_work.clone();
    let candidates = source::failure_candidates(project);
    for task in candidates {
        if !project.planning_work.iter().any(|w| w.uid == task.uid) {
            project.planning_work.push(task);
        }
    }
    for index in 0..project.planning_work.len() {
        let Some(recovery) = project.planning_work[index].operational_recovery.clone() else {
            continue;
        };
        if project
            .cancelled_work
            .contains(&crate::persistence::cancelled_work::planning_id(
                &project.planning_work[index].uid,
            ))
        {
            continue;
        }
        if project.planning_work[index].status == WorkStatus::Done {
            continue;
        }
        if recovery.autonomous_investigation {
            if investigation::advance(project, index) {
                return;
            }
            continue;
        }
        if project
            .cancelled_work
            .contains(&crate::persistence::cancelled_work::planning_id(
                &recovery.source_uid,
            ))
            || recovery.expected_feature_uid.as_ref().is_some_and(|uid| {
                project
                    .cancelled_work
                    .contains(&crate::persistence::cancelled_work::planning_id(uid))
            })
        {
            project.planning_work[index].status = WorkStatus::Done;
            project.planning_work[index].detail =
                "Recovery stopped because the original task or feature was cancelled.".into();
            continue;
        }
        let source = source(project, &recovery.source_key);
        if source
            .as_ref()
            .is_some_and(|current| current.uid != recovery.source_uid)
        {
            project.planning_work[index].status = WorkStatus::NeedsAttention;
            project.planning_work[index].detail = format!(
                "Recovery blocked: conversation {} now refers to a different task UID. Preserve the original task {} and reconcile its conversation identity before checking reply outcomes.",
                recovery.source_key, recovery.source_uid
            );
            continue;
        }
        let messages = project.task_chats.messages.get(&recovery.source_key);
        let latest = messages.and_then(|messages| {
            messages
                .iter()
                .rposition(|m| m.role == ChatRole::User && !m.clarification)
                .map(|i| (&messages[i], &messages[i + 1..]))
        });
        let result = match latest {
            Some((user, _)) if user.id != recovery.reply_id => Some((WorkStatus::Done, "Superseded by a newer saved answer. Its recovery is tracked separately.".into())),
            Some((_, after)) if after.iter().any(|m| m.role == ChatRole::System &&
                (m.id.starts_with("internal-task-reply-outcome:applied:") || m.id.starts_with("internal-retry:planning-write-applied:"))) =>
                Some((WorkStatus::Done, "Verified: the original saved answer was applied through the normal guarded planning write.".into())),
            None => Some((WorkStatus::NeedsAttention, "Recovery blocked: the original saved conversation is unavailable. Restore the conversation before retrying; do not invent a replacement answer.".into())),
            _ => None,
        };
        if let Some((status, detail)) = result {
            project.planning_work[index].status = status;
            project.planning_work[index].detail = detail;
            continue;
        }
        if project.task_turns.contains_key(&recovery.source_key)
            || project
                .task_chats
                .has_active_reply_claim(&recovery.source_key)
        {
            project.planning_work[index].status = WorkStatus::InProgress;
            project.planning_work[index].detail = ACTIVE_REPLY_DETAIL.into();
            continue;
        }
        let cancelled =
            project
                .cancelled_work
                .contains(&crate::persistence::cancelled_work::planning_id(
                    &recovery.source_uid,
                ));
        if !cancelled
            && source.as_ref().is_some_and(|s| {
                s.uid == recovery.source_uid && s.feature_uid == recovery.expected_feature_uid
            })
            && recovery.expected_feature_uid.as_ref().is_some_and(|uid| {
                !project.state.active_features.iter().any(|(_, body)| {
                    crate::domain::ArtifactIdentity::from_markdown(body)
                        .ok()
                        .flatten()
                        .is_some_and(|i| i.uid == *uid)
                })
            })
            && recovery.repair_attempts < 2
            && recovery.repair_error.is_none()
            && chrono::Utc::now().timestamp_millis() >= recovery.next_attempt_ms
        {
            native::start(project, index);
            return;
        }
        let result = (|| {
            let Some(source) = &source else {
                return Some((WorkStatus::NeedsAttention,
                "Recovery blocked: the original source task no longer exists. Its saved answer is preserved; restore or explicitly reconcile the source task before retrying.".into()));
            };
            if source.uid != recovery.source_uid
                || source.feature_uid != recovery.expected_feature_uid
            {
                return Some((
                    WorkStatus::NeedsAttention,
                    format!(
                        "Recovery blocked: source identity changed. Expected task {} and feature {:?}; found task {} and feature {:?}. Resolve this identity conflict before replay; matching display IDs do not authorize reassociation.",
                        recovery.source_uid,
                        recovery.expected_feature_uid,
                        source.uid,
                        source.feature_uid
                    ),
                ));
            }
            if let Some(uid) = &recovery.expected_feature_uid {
                let matches = project
                    .state
                    .active_features
                    .iter()
                    .filter(|(_, text)| {
                        crate::domain::ArtifactIdentity::from_markdown(text)
                            .ok()
                            .flatten()
                            .is_some_and(|i| i.uid == *uid)
                    })
                    .count();
                if matches != 1 {
                    return Some((
                        WorkStatus::NeedsAttention,
                        format!(
                            "Recovery blocked: feature UID {uid} has {matches} active matches. Refresh or restore the exact feature before replay. Original diagnostic: {}. Native investigation: {}",
                            recovery.evidence,
                            recovery
                                .repair_error
                                .as_deref()
                                .unwrap_or("no verified repair available")
                        ),
                    ));
                }
            }
            if project
                .cancelled_work
                .contains(&crate::persistence::cancelled_work::planning_id(
                    &source.uid,
                ))
            {
                return Some((
                    WorkStatus::Done,
                    "Recovery stopped because the source task was cancelled.".into(),
                ));
            }
            None
        })();
        if let Some((status, detail)) = result {
            project.planning_work[index].status = status;
            project.planning_work[index].detail = detail;
            continue;
        }
        if project
            .task_chats
            .has_pending_user_reply(&recovery.source_key)
        {
            let task = &mut project.planning_work[index];
            if let Some(blocker) = project
                .activity
                .saved_reply_dispatch_blockers
                .get(&recovery.source_key)
            {
                task.status = WorkStatus::NeedsAttention;
                task.detail =
                    format!("Manager recovery cannot dispatch the saved answer: {blocker}");
            } else {
                task.status = WorkStatus::Todo;
                task.detail = "The saved answer is eligible for automatic dispatch; no worker currently owns it. Manager retains the queued recovery task.".into();
            }
            continue;
        }
        let authorized = messages.is_some_and(|messages| {
            messages
                .iter()
                .any(|m| m.id == format!("internal-manager-recovery:{}", recovery.reply_id))
        });
        if authorized || recovery.attempts >= 3 {
            project.planning_work[index].status = WorkStatus::NeedsAttention;
            project.planning_work[index].detail = format!(
                "Recovery attempt did not produce an application receipt. Manager retains ownership; investigate the attached diagnostic and repair the underlying execution or product failure before retrying. {}",
                recovery.evidence
            );
            continue;
        }
        // Checkpoint ownership before touching the independent conversation store.
        let now = chrono::Utc::now().timestamp_millis();
        if now < recovery.next_attempt_ms {
            continue;
        }
        let pending = project.planning_work[index]
            .operational_recovery
            .as_mut()
            .unwrap();
        pending.attempts += 1;
        pending.next_attempt_ms = now + 30_000;
        project.planning_work[index].status = WorkStatus::InProgress;
        project.planning_work[index].detail = ACTIVE_REPLY_DETAIL.into();
        if project.save_planning_work().is_err() {
            project.planning_work = original;
            return;
        }
        match project.task_chats.authorize_manager_recovery(&project.chat_slug, &recovery.source_key, &recovery.source_uid, &recovery.reply_id) {
            Ok(true) => project.activity.pending.push("Manager triage scheduled a guarded recovery of a saved task answer.".into()),
            Ok(false) => project.queue.last_error = "Manager recovery was not authorized because the saved answer or its recovery ownership changed; refreshing will re-evaluate it.".into(),
            Err(error) => project.queue.last_error = format!("Manager recovery could not save its authorization: {error}"),
        }
        return;
    }
    if project.planning_work != original {
        if let Err(error) = project.save_planning_work() {
            project.planning_work = original;
            project.queue.last_error =
                format!("Manager triage could not persist recovery ownership: {error}");
        } else {
            project.activity.pending.push("Manager operational triage updated recovery tasks. Saved answers remain application-owned; inspect each triage task for its next action or blocker.".into());
        }
    }
}
