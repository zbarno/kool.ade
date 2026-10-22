use super::{KooladeApp, Screen};

impl KooladeApp {
    pub(super) fn publish_lifecycle_pr(&mut self, ticket: &str) {
        let validation = match &self.screen {
            Screen::Connected(project) => {
                match project
                    .task_documents
                    .iter()
                    .find(|document| document.path == ticket)
                    .and_then(|document| document.identity.as_ref())
                    .map(|identity| identity.uid.as_str())
                {
                    Some(planning_item_uid) => {
                        let lifecycle = crate::core::lifecycle::LifecycleStore::new(
                            &project.state.planning_store,
                        );
                        match lifecycle.task_for_planning_item(planning_item_uid) {
                        Ok(Some(task)) if task.phase == crate::core::lifecycle::TaskPhase::Done => {
                            match task.pr_work_item_uid.as_deref() {
                                Some(work_uid) => lifecycle
                                    .load_pr_preparation(work_uid)
                                    .map_err(|error| error.to_string())
                                    .and_then(|work| {
                                        if matches!(work.status,
                                            crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization
                                            | crate::core::lifecycle::PullRequestPreparationStatus::NeedsAttention)
                                        {
                                            Ok(())
                                        } else {
                                            Err(format!("PR work item is {:?}, not awaiting authorization or retry", work.status))
                                        }
                                    }),
                                None => Err("completed lifecycle task has no PR work item".into()),
                            }
                        }
                        Ok(Some(task)) => Err(format!(
                            "lifecycle task is {:?}; QA and specification reconciliation must finish before PR authorization",
                            task.phase
                        )),
                        Ok(None) => Err("lifecycle task record is missing".into()),
                        Err(error) => Err(error.to_string()),
                    }
                    }
                    None => Err(
                        "task has no stable planning identity; PR authorization is unavailable"
                            .to_owned(),
                    ),
                }
            }
            Screen::Welcome => Err("connect a project before authorizing publication".into()),
        };
        match validation {
            Ok(()) => self.start_lifecycle_pr_publication(ticket.to_owned()),
            Err(error) => self.toasts.warning(error),
        }
    }
}
