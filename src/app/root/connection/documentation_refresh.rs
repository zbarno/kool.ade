use crate::{
    app::session::Project,
    core::{
        lifecycle::LifecycleStore,
        planning_work::{Work, WorkKind},
    },
};

#[cfg(test)]
#[path = "documentation_refresh/tests.rs"]
mod tests;

pub(super) fn record(project: &mut Project, work: Option<&Work>, message_uid: &str) {
    if let Err(error) = enqueue(&project.state.planning_store, work, message_uid) {
        project.activity.pending.push(format!(
            "Documentation refresh trigger could not be recorded: {error}"
        ));
    }
}

fn enqueue(
    store: &crate::artifacts::planning_store::PlanningStore,
    work: Option<&Work>,
    message_uid: &str,
) -> Result<(), String> {
    let Some(work) = work.filter(|work| work.kind == WorkKind::DocumentationRefresh) else {
        return Ok(());
    };
    let Some(feature_uid) = work.feature_uid.as_deref() else {
        return Ok(());
    };
    LifecycleStore::new(store)
        .enqueue_manual_spec_refresh_correlated(feature_uid, message_uid, Some(message_uid))
        .map(|_| ())
        .map_err(|error| error.to_string())
}
