use super::Project;
use crate::core::{
    implementation::{Implementation, ImplementationStatus, PullRequestState},
    planning_work::WorkStatus,
};
use linkage::{execution_linkage, task_record_matches_previous};

mod linkage;

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
                if !task_record_matches_previous(document.task_state.as_ref(), previous, &metadata)
                {
                    return None;
                }
                let expected_revision = document
                    .task_state
                    .as_ref()
                    .map_or(0, |task_state| task_state.revision);
                let (status, execution_status) = shared_status(state);
                let linkage = execution_linkage(&metadata, state);
                Some((
                    index,
                    metadata,
                    expected_revision,
                    status,
                    execution_status,
                    linkage,
                ))
            })
            .collect::<Vec<_>>();

        for (index, metadata, expected_revision, status, execution_status, linkage) in updates {
            match crate::artifacts::task_docs::update_task_execution_status(
                &self.state.planning_store,
                &metadata,
                expected_revision,
                status,
                execution_status,
                &linkage,
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
    use super::*;

    fn metadata(repository_id: &str) -> crate::artifacts::task_docs::TaskMetadata {
        let identity = crate::domain::ArtifactIdentity {
            uid: uuid::Uuid::new_v4().to_string(),
            display_id: "TASK-001".into(),
            title: "Task".into(),
            parent_uid: Some(uuid::Uuid::new_v4().to_string()),
        };
        crate::artifacts::task_docs::TaskMetadata::new(&identity, repository_id, vec![]).unwrap()
    }

    fn implementation(status: ImplementationStatus) -> Implementation {
        serde_json::from_value(serde_json::json!({
            "ticket": "task.md",
            "ticket_text": "",
            "branch": "koolade/task",
            "base": "main",
            "base_commit": "0000000000000000000000000000000000000000",
            "task_repository": "/tmp/koolade-task",
            "status": status,
            "detail": "",
            "pr_url": null,
            "verified_head": null,
            "repository_id": "root"
        }))
        .unwrap()
    }

    #[test]
    fn stale_local_status_cannot_replace_a_newer_shared_task_status() {
        let shared = crate::artifacts::task_docs::TaskState {
            batch_uid: uuid::Uuid::new_v4().to_string(),
            repository_id: "root".into(),
            dependency_uids: Vec::new(),
            status: WorkStatus::Done,
            execution_status: Some("completed".into()),
            source_branch: None,
            source_sha: None,
            destination_branch: None,
            implementation_branch: None,
            publication_status: crate::artifacts::task_docs::ExecutionPublicationStatus::Planned,
            pull_requests: Vec::new(),
            legacy_linkage: false,
            revision: 4,
        };

        assert!(!task_record_matches_previous(
            Some(&shared),
            &implementation(ImplementationStatus::Implementing),
            &metadata("root")
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
            source_branch: None,
            source_sha: None,
            destination_branch: None,
            implementation_branch: None,
            publication_status: crate::artifacts::task_docs::ExecutionPublicationStatus::Planned,
            pull_requests: Vec::new(),
            legacy_linkage: false,
            revision: 1,
        };

        assert!(task_record_matches_previous(
            Some(&shared),
            &implementation(ImplementationStatus::Preparing),
            &metadata("root")
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
            source_branch: None,
            source_sha: None,
            destination_branch: None,
            implementation_branch: None,
            publication_status: crate::artifacts::task_docs::ExecutionPublicationStatus::Planned,
            pull_requests: Vec::new(),
            legacy_linkage: false,
            revision: 3,
        };

        assert!(!task_record_matches_previous(
            Some(&shared),
            &implementation(ImplementationStatus::Implementing),
            &metadata("root")
        ));
    }

    #[test]
    fn shared_linkage_keeps_source_target_and_closed_vs_merged_pr_evidence() {
        use crate::artifacts::task_docs::ExecutionPublicationStatus as LinkStatus;

        let metadata = metadata("api");
        let head = "a".repeat(40);
        let source = "b".repeat(40);
        let mut closed = implementation(ImplementationStatus::PullRequestClosed);
        closed.repository_id = Some("api".into());
        closed.source_ref = Some("release/1.2".into());
        closed.source_commit = Some(source.clone());
        closed.destination_branch = Some("develop".into());
        closed.base = "develop".into();
        closed.pr_url = Some("https://github.com/fixture/api/pull/27".into());
        closed.verified_head = Some(head.clone());
        closed.pr_state = Some(PullRequestState::Closed);
        closed.pr_details = Some(crate::core::implementation::PullRequestDetails {
            number: 27,
            head_sha: head.clone(),
            base_branch: "develop".into(),
        });
        let closed_link = execution_linkage(&metadata, &closed);
        assert_eq!(closed_link.repository_id, "api");
        assert_eq!(closed_link.source_branch.as_deref(), Some("release/1.2"));
        assert_eq!(closed_link.source_sha.as_deref(), Some(source.as_str()));
        assert_eq!(closed_link.destination_branch.as_deref(), Some("develop"));
        assert_eq!(closed_link.publication_status, LinkStatus::ClosedUnmerged);
        assert_eq!(closed_link.pull_requests[0].number, 27);
        assert_eq!(
            closed_link.pull_requests[0].status,
            LinkStatus::ClosedUnmerged
        );

        let mut merged = closed;
        merged.status = ImplementationStatus::Completed;
        merged.pr_state = Some(PullRequestState::Merged);
        merged.merged_commit = Some("c".repeat(40));
        let merged_link = execution_linkage(&metadata, &merged);
        assert_eq!(merged_link.publication_status, LinkStatus::Merged);
        assert_eq!(merged_link.pull_requests[0].status, LinkStatus::Merged);
        assert_eq!(
            merged_link.pull_requests[0].merge_sha.as_deref(),
            Some("c".repeat(40).as_str())
        );
    }
}
