use crate::{
    app::session::Project,
    core::planning_work::{WorkKind, WorkStatus},
};

pub(crate) fn mark_repair_failed(project: &mut Project, ticket: &str, reason: &str) {
    let Some(context) = project
        .implementation_states
        .get(ticket)
        .and_then(|state| state.manager_repair_context.clone())
    else {
        return;
    };
    let Some(work) = project.planning_work.iter_mut().find(|work| {
        work.kind == WorkKind::CodeReviewDisposition
            && work.parent_uid.as_deref() == Some(context.task_uid.as_str())
            && work.uid == context.work_id
    }) else {
        return;
    };
    if work.status == WorkStatus::Done {
        return;
    }
    work.status = WorkStatus::NeedsAttention;
    work.detail.push_str(&format!(
        "\n\nThe scoped repair worker stopped before producing a fresh independent review: {reason}"
    ));
    if let Err(error) = project.save_planning_work() {
        project.activity.pending.push(format!(
            "The scoped repair worker stopped for task {ticket}, but its board attention state could not be saved: {error}"
        ));
    }
}
