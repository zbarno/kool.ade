//! Checkpoint Manager-owned reply status before capturing a model snapshot.
use super::*;

pub(crate) fn prepare_reply_dispatch(
    project: &mut Project,
    key: &str,
    reply: &str,
) -> Result<(), String> {
    let Some(index) = project.planning_work.iter().position(|work| {
        work.operational_recovery.as_ref().is_some_and(|r| {
            !r.autonomous_investigation && r.source_key == key && r.reply_id == reply
        })
    }) else {
        return Ok(());
    };
    let task = &project.planning_work[index];
    let recovery = task.operational_recovery.as_ref().unwrap();
    let current = source(project, key).ok_or("The original recovery task is unavailable")?;
    if current.uid != recovery.source_uid || current.feature_uid != recovery.expected_feature_uid {
        return Err("The saved answer's original task or feature identity changed; reconcile that identity before dispatch.".into());
    }
    if [&task.uid, &recovery.source_uid].iter().any(|uid| {
        project
            .cancelled_work
            .contains(&crate::persistence::cancelled_work::planning_id(uid))
    }) || recovery.expected_feature_uid.as_ref().is_some_and(|uid| {
        project
            .cancelled_work
            .contains(&crate::persistence::cancelled_work::planning_id(uid))
    }) {
        return Err("This recovery or its original source was cancelled.".into());
    }
    if task.status == WorkStatus::InProgress && task.detail == ACTIVE_REPLY_DETAIL {
        return Ok(());
    }
    let original = project.planning_work.clone();
    project.planning_work[index].status = WorkStatus::InProgress;
    project.planning_work[index].detail = ACTIVE_REPLY_DETAIL.into();
    if let Err(error) = project.save_planning_work() {
        project.planning_work = original;
        return Err(format!(
            "Recovery ownership could not be checkpointed before dispatch: {error}"
        ));
    }
    Ok(())
}
