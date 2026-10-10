use super::Project;
use crate::core::{
    implementation::{Implementation, ImplementationStatus, PullRequestState},
    planning_work::WorkStatus,
};

impl Project {
    pub(super) fn sync_task_status_records(
        &mut self,
        changed_baselines: &std::collections::BTreeMap<
            String,
            crate::core::implementation::Implementation,
        >,
    ) {
        let updates = self
            .task_documents
            .iter()
            .enumerate()
            .filter_map(|(index, document)| {
                let previous = changed_baselines.get(&document.path)?;
                let metadata = document.metadata.clone()?;
                let state = self.implementation_states.get(&document.path)?;
                if state
                    .task_uid
                    .as_deref()
                    .is_some_and(|uid| uid != metadata.uid)
                {
                    return None;
                }
                let (previous_status, previous_execution_status) = shared_status(previous);
                if !task_record_matches_previous(
                    document.task_state.as_ref(),
                    previous_status,
                    previous_execution_status,
                ) {
                    return None;
                }
                let expected_revision = document
                    .task_state
                    .as_ref()
                    .map_or(0, |task_state| task_state.revision);
                let (status, execution_status) = shared_status(state);
                Some((index, metadata, expected_revision, status, execution_status))
            })
            .collect::<Vec<_>>();

        for (index, metadata, expected_revision, status, execution_status) in updates {
            match crate::artifacts::task_docs::update_task_execution_status(
                &self.state.planning_store,
                &metadata,
                expected_revision,
                status,
                execution_status,
            ) {
                Ok(task_state) => self.task_documents[index].task_state = Some(task_state),
                Err(error) => {
                    let note = format!(
                        "Task status for {} could not be saved: {error}",
                        self.task_documents[index].path
                    );
                    if !self.activity.pending.contains(&note) {
                        self.activity.pending.push(note);
                    }
                }
            }
        }
    }
}

fn task_record_matches_previous(
    task_state: Option<&crate::artifacts::task_docs::TaskState>,
    previous_status: WorkStatus,
    previous_execution_status: &str,
) -> bool {
    let Some(task_state) = task_state else {
        return true;
    };
    (task_state.status == previous_status
        && task_state.execution_status.as_deref() == Some(previous_execution_status))
        || (task_state.status == WorkStatus::Todo
            && task_state.execution_status.is_none()
            && task_state.revision == 1)
}

fn shared_status(state: &Implementation) -> (WorkStatus, &'static str) {
    match state.pr_state {
        Some(PullRequestState::Merged) => (WorkStatus::Done, "completed"),
        Some(PullRequestState::Closed) => (WorkStatus::NeedsAttention, "pull_request_closed"),
        Some(PullRequestState::Open) => (WorkStatus::InReview, "awaiting_review"),
        None => match state.status {
            ImplementationStatus::Preparing
            | ImplementationStatus::Implementing
            | ImplementationStatus::Verifying
            | ImplementationStatus::Publishing
            | ImplementationStatus::WaitingForIndependentChecks
            | ImplementationStatus::WaitingToMerge => {
                (WorkStatus::InProgress, state.status.wire_name())
            }
            ImplementationStatus::ReadyToPublish
            | ImplementationStatus::AwaitingApproval
            | ImplementationStatus::ChangesRequested
            | ImplementationStatus::AwaitingReview => {
                (WorkStatus::InReview, state.status.wire_name())
            }
            ImplementationStatus::Completed => (WorkStatus::Done, state.status.wire_name()),
            ImplementationStatus::Blocked
            | ImplementationStatus::PullRequestClosed
            | ImplementationStatus::Interrupted => {
                (WorkStatus::NeedsAttention, state.status.wire_name())
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::task_record_matches_previous;
    use crate::core::planning_work::WorkStatus;

    #[test]
    fn stale_local_status_cannot_replace_a_newer_shared_task_status() {
        let shared = crate::artifacts::task_docs::TaskState {
            batch_uid: uuid::Uuid::new_v4().to_string(),
            repository_id: "root".into(),
            dependency_uids: Vec::new(),
            status: WorkStatus::Done,
            execution_status: Some("completed".into()),
            revision: 4,
        };

        assert!(!task_record_matches_previous(
            Some(&shared),
            WorkStatus::InProgress,
            "implementing"
        ));
    }

    #[test]
    fn first_local_implementation_transition_can_advance_a_todo_record() {
        let shared = crate::artifacts::task_docs::TaskState {
            batch_uid: uuid::Uuid::new_v4().to_string(),
            repository_id: "root".into(),
            dependency_uids: Vec::new(),
            status: WorkStatus::Todo,
            execution_status: None,
            revision: 1,
        };

        assert!(task_record_matches_previous(
            Some(&shared),
            WorkStatus::InProgress,
            "preparing"
        ));
    }

    #[test]
    fn local_status_does_not_replace_a_task_explicitly_reset_to_todo() {
        let shared = crate::artifacts::task_docs::TaskState {
            batch_uid: uuid::Uuid::new_v4().to_string(),
            repository_id: "root".into(),
            dependency_uids: Vec::new(),
            status: WorkStatus::Todo,
            execution_status: None,
            revision: 3,
        };

        assert!(!task_record_matches_previous(
            Some(&shared),
            WorkStatus::InProgress,
            "implementing"
        ));
    }
}
