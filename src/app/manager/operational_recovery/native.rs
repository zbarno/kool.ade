//! Native triage uses application storage without depending on a model or Docker.
use super::*;
use std::sync::mpsc;

pub(crate) struct NativeWorker {
    uid: String,
    result: mpsc::Receiver<anyhow::Result<crate::core::state::PlannerState>>,
}

pub(super) fn start(project: &mut Project, index: usize) {
    let original = project.planning_work.clone();
    let task = &mut project.planning_work[index];
    let recovery = task.operational_recovery.as_mut().unwrap();
    let feature_uid = recovery.expected_feature_uid.clone().unwrap();
    recovery.repair_attempts += 1;
    recovery.next_attempt_ms = chrono::Utc::now().timestamp_millis() + 30_000;
    task.status = WorkStatus::InProgress;
    task.detail = "Native triage is inspecting verified private feature snapshots and attempting an exact-identity repair. It does not require Docker or a model provider.".into();
    let uid = task.uid.clone();
    if let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        project.queue.last_error = format!("Native triage could not checkpoint ownership: {error}");
        return;
    }
    let state = project.state.clone();
    let recovery = project
        .planning_work
        .iter()
        .find(|task| task.uid == uid)
        .unwrap()
        .operational_recovery
        .clone()
        .unwrap();
    let slug = project.chat_slug.clone();
    let task_uid = uid.clone();
    let (send, result) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| -> anyhow::Result<_> {
            let _reply_lock =
                crate::persistence::task_chats::TaskChats::lock_failed_reply_for_recovery(
                    &slug,
                    &recovery.source_key,
                    &recovery.source_uid,
                    &recovery.reply_id,
                )
                .map_err(anyhow::Error::msg)?
                .ok_or_else(|| anyhow::Error::new(Deferred))?;
            let state = crate::core::state::PlannerState::load_with_store(
                &state.repo_root,
                &state.planning_store,
            )?;
            let cancelled = crate::persistence::cancelled_work::load(&state.planning_store)?;
            anyhow::ensure!(
                ![&recovery.source_uid, &task_uid, &feature_uid]
                    .iter()
                    .any(|uid| cancelled
                        .contains(&crate::persistence::cancelled_work::planning_id(uid))),
                "Native recovery was cancelled"
            );
            let work = crate::core::planning_work::load(&state.planning_store)?;
            let source_matches = work.iter().any(|w| {
                w.uid == recovery.source_uid && w.feature_uid.as_ref() == Some(&feature_uid)
            }) || state.items.iter().any(|i| {
                i.uid.as_deref() == Some(recovery.source_uid.as_str())
                    && i.feature_uid.as_ref() == Some(&feature_uid)
            });
            anyhow::ensure!(
                source_matches,
                "The original source feature link changed before native triage"
            );
            if state.active_features.iter().any(|(_, body)| {
                crate::domain::ArtifactIdentity::from_markdown(body)
                    .ok()
                    .flatten()
                    .is_some_and(|i| i.uid == feature_uid)
            }) {
                return Ok(state);
            }
            crate::core::apply::restore_missing_feature(&state, &feature_uid)
        })();
        let _ = send.send(result);
    });
    project.activity.operational_recovery_worker = Some(NativeWorker { uid, result });
}

pub(super) fn poll(project: &mut Project) -> bool {
    let Some(worker) = &project.activity.operational_recovery_worker else {
        return false;
    };
    let result = match worker.result.try_recv() {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => return true,
        Err(mpsc::TryRecvError::Disconnected) => Err(anyhow::anyhow!(
            "Native triage worker stopped without a result"
        )),
    };
    let uid = worker.uid.clone();
    project.activity.operational_recovery_worker = None;
    // Reload the durable task record; a concurrent cancel or edit must survive.
    match crate::core::planning_work::load(&project.state.planning_store) {
        Ok(work) => project.planning_work = work,
        Err(error) => {
            project.queue.last_error =
                format!("Native triage result could not reload ownership: {error}");
            return true;
        }
    }
    let Some(task) = project.planning_work.iter_mut().find(|w| w.uid == uid) else {
        return true;
    };
    let recovery = task.operational_recovery.as_mut().unwrap();
    let cancelled = crate::persistence::cancelled_work::load(&project.state.planning_store);
    let revision = project.state.planning_store.revision();
    match result {
        Ok(state)
            if revision
                .as_ref()
                .is_ok_and(|revision| revision == &state.baseline_planning_revision)
                && cancelled.as_ref().is_ok_and(|ids| {
                    ![
                        Some(&uid),
                        Some(&recovery.source_uid),
                        recovery.expected_feature_uid.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                    .any(|uid| ids.contains(&crate::persistence::cancelled_work::planning_id(uid)))
                }) =>
        {
            recovery.repair_error = None;
            task.detail = "Native repair restored and verified the exact feature UID. Manager will now revalidate the saved answer before replay.".into();
            project.state = state;
        }
        Ok(_) => {
            task.detail = "Native repair finished, but the project changed before adoption. Manager retained current state and will re-evaluate after refresh.".into();
        }
        Err(error) if error.is::<Deferred>() => {
            task.status = WorkStatus::Todo;
            task.detail = "Native triage deferred to an active or changed saved answer. Manager will revalidate after the recovery cooldown.".into();
        }
        Err(error) => {
            recovery.repair_error = Some(error.to_string());
            task.detail = format!(
                "Native investigation could not safely repair the feature: {error}. The original answer remains saved; explicit identity reconciliation is required."
            );
            task.status = WorkStatus::NeedsAttention;
        }
    }
    if let Err(error) = project.save_planning_work() {
        project.queue.last_error = format!("Native triage result could not be recorded: {error}");
    }
    project.activity.pending.push("Manager native triage completed; inspect its durable task for the verified repair or exact blocker.".into());
    true
}

#[derive(Debug)]
struct Deferred;
impl std::fmt::Display for Deferred {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Native triage deferred because saved-answer ownership changed")
    }
}
impl std::error::Error for Deferred {}
