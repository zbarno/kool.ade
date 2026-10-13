use crate::{app::session::Project, core::lifecycle::PullRequestPreparationStatus};

pub(super) fn for_ticket(
    project: &Project,
    ticket: &str,
) -> (
    Option<String>,
    Option<PullRequestPreparationStatus>,
    Option<String>,
) {
    let Some(planning_item_uid) = project
        .task_documents
        .iter()
        .find(|document| document.path == ticket)
        .and_then(|document| document.identity.as_ref())
        .map(|identity| identity.uid.as_str())
    else {
        return (None, None, None);
    };
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
    match lifecycle.task_for_planning_item(planning_item_uid) {
        Ok(Some(task)) => {
            let Some(uid) = task.pr_work_item_uid else {
                return (None, None, None);
            };
            match lifecycle.load_pr_preparation(&uid) {
                Ok(work) => (Some(uid), Some(work.status), None),
                Err(error) => (Some(uid), None, Some(error.to_string())),
            }
        }
        Ok(None) => (None, None, None),
        Err(error) => (None, None, Some(error.to_string())),
    }
}
