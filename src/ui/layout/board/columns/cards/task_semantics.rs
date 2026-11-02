use crate::{artifacts::task_docs::TaskDocument, core::lifecycle::TaskPhase};

pub(super) fn kind_label(
    document: &TaskDocument,
    phase: Option<TaskPhase>,
    pr_status: Option<crate::core::lifecycle::PullRequestPreparationStatus>,
) -> &'static str {
    if let Some(status) = pr_status {
        use crate::core::lifecycle::PullRequestPreparationStatus as Status;
        return match status {
            Status::AwaitingAuthorization | Status::Publishing | Status::NeedsAttention => {
                "Generate PR"
            }
            Status::Opened | Status::ClosedUnmerged | Status::Merged => "Pull Request",
        };
    }
    if document
        .metadata
        .as_ref()
        .is_some_and(|metadata| metadata.parent_task_uid.is_some())
    {
        return "Subtask";
    }
    match phase {
        Some(TaskPhase::AwaitingReview | TaskPhase::Repairing | TaskPhase::ReReview) => {
            "Code Review"
        }
        Some(TaskPhase::AwaitingRuntimeQa) => "Runtime QA",
        Some(TaskPhase::AwaitingSpecReconciliation) => "Spec Reconciliation",
        Some(TaskPhase::PrPending) => "Generate PR",
        Some(TaskPhase::PrOpened | TaskPhase::PrMerged) => "Pull Request",
        _ => "Standalone Task",
    }
}

pub(super) fn next_step(
    phase: TaskPhase,
    pr_status: Option<crate::core::lifecycle::PullRequestPreparationStatus>,
) -> &'static str {
    if let Some(status) = pr_status {
        use crate::core::lifecycle::PullRequestPreparationStatus as Status;
        return match status {
            Status::AwaitingAuthorization => "Review and authorize PR preparation",
            Status::Publishing => "Opening the parent pull request",
            Status::NeedsAttention => "Review PR failure in Task Details",
            Status::Opened => "Pull request is open",
            Status::ClosedUnmerged => "Pull request closed without merging",
            Status::Merged => "Pull request merged",
        };
    }
    match phase {
        TaskPhase::Ready => "Ready to implement",
        TaskPhase::Blocked => "Blocked by a prerequisite",
        TaskPhase::Running => "Implementation in progress",
        TaskPhase::AwaitingReview => "Waiting for independent code review",
        TaskPhase::Repairing => "Manager-directed repair in progress",
        TaskPhase::ReReview => "Waiting for independent review after repair",
        TaskPhase::AwaitingRuntimeQa => "Waiting for independent Runtime QA",
        TaskPhase::AwaitingSpecReconciliation => "Reconciling the specification",
        TaskPhase::Done => "Implementation complete",
        TaskPhase::PrPending => "Prepare the parent pull request",
        TaskPhase::PrOpened => "Pull request is open",
        TaskPhase::PrMerged => "Merged",
        TaskPhase::Failed => "Needs attention",
        TaskPhase::Cancelled => "Cancelled",
    }
}

pub(super) fn pr_status_label(
    status: crate::core::lifecycle::PullRequestPreparationStatus,
) -> &'static str {
    use crate::core::lifecycle::PullRequestPreparationStatus as Status;
    match status {
        Status::AwaitingAuthorization => "PR authorization needed",
        Status::Publishing => "Opening PR",
        Status::NeedsAttention => "PR needs attention",
        Status::Opened => "PR open",
        Status::ClosedUnmerged => "PR closed unmerged",
        Status::Merged => "PR merged",
    }
}

pub(super) fn pr_action_label(
    status: crate::core::lifecycle::PullRequestPreparationStatus,
) -> Option<&'static str> {
    use crate::core::lifecycle::PullRequestPreparationStatus as Status;
    match status {
        Status::AwaitingAuthorization => Some("Review PR"),
        Status::Publishing => Some("View progress"),
        Status::NeedsAttention => Some("Review PR failure"),
        Status::Opened => Some("Open PR"),
        Status::ClosedUnmerged => Some("Review closed PR"),
        Status::Merged => None,
    }
}

pub(super) fn gate_track(
    task: Option<&crate::core::lifecycle::TaskRecord>,
    pr_status: Option<crate::core::lifecycle::PullRequestPreparationStatus>,
) -> Option<String> {
    let task = task?;
    let pr = pr_status.map_or_else(
        || "not recorded".to_owned(),
        |status| {
            pr_status_label(status)
                .strip_prefix("PR ")
                .unwrap_or(pr_status_label(status))
                .to_owned()
        },
    );
    let merged = if pr_status == Some(crate::core::lifecycle::PullRequestPreparationStatus::Merged)
        || task.phase == TaskPhase::PrMerged
    {
        "recorded"
    } else {
        "not recorded"
    };
    Some(format!(
        "Review {} · Runtime QA {} · Spec sync {} · PR {pr} · Merge {merged}",
        evidence_label(task.code_review.as_ref()),
        evidence_label(task.runtime_qa.as_ref()),
        evidence_label(task.spec_reconciliation.as_ref()),
    ))
}

fn evidence_label(receipt: Option<&crate::core::lifecycle::GateReceipt>) -> &'static str {
    use crate::core::lifecycle::GateDecision;
    match receipt.map(|receipt| receipt.decision) {
        Some(GateDecision::Approved) => "approved",
        Some(GateDecision::ChangesRequested) => "changes requested",
        Some(GateDecision::Rejected) => "rejected",
        Some(GateDecision::NotApplicable) => "not applicable",
        None => "not recorded",
    }
}

pub(super) fn board_column(
    status: Option<crate::core::lifecycle::PullRequestPreparationStatus>,
    fallback: usize,
) -> usize {
    use crate::core::lifecycle::PullRequestPreparationStatus as Status;
    match status {
        Some(Status::AwaitingAuthorization | Status::NeedsAttention | Status::ClosedUnmerged) => 3,
        Some(Status::Publishing) => 1,
        Some(Status::Opened | Status::Merged) | None => fallback,
    }
}

pub(super) fn task_board_column(
    phase: Option<TaskPhase>,
    pr_status: Option<crate::core::lifecycle::PullRequestPreparationStatus>,
    fallback: usize,
) -> usize {
    if phase == Some(TaskPhase::Done) {
        4
    } else {
        board_column(pr_status, fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        board_column, gate_track, kind_label, next_step, pr_action_label, task_board_column,
    };
    use crate::core::lifecycle::TaskPhase;

    fn lifecycle_task(phase: TaskPhase) -> crate::core::lifecycle::TaskRecord {
        crate::core::lifecycle::TaskRecord {
            schema_version: 1,
            task_uid: "task-uid".into(),
            feature_uid: "feature-uid".into(),
            planning_item_uid: Some("planning-uid".into()),
            task_spec_sha256: None,
            parent_task_uid: None,
            subtask_uids: Vec::new(),
            dependency_task_uids: Vec::new(),
            approved_requirement_ids: Vec::new(),
            phase,
            revision: 1,
            approved_plan_sha256: "a".repeat(64),
            implementation_sha256: None,
            reviewed_code_head_sha: None,
            reviewed_code_subject_sha256: None,
            code_review: None,
            review_history: Vec::new(),
            repair_attempts: 0,
            runtime_qa: None,
            qa_execution: None,
            spec_reconciliation: None,
            spec_reconciliation_blocker: None,
            spec_reconciliation_proposal_sha256: None,
            spec_reconciliation_spec_sha256: None,
            spec_reconciliation_proposal_json: None,
            pr_work_item_uid: None,
        }
    }

    #[test]
    fn lifecycle_cards_name_the_current_review_qa_spec_and_pr_work() {
        let document = crate::artifacts::task_docs::TaskDocument {
            path: "tasks/work.md".into(),
            title: "Work".into(),
            text: String::new(),
            metadata: None,
            task_state: None,
            metadata_error: None,
            identity: None,
        };
        assert_eq!(
            kind_label(&document, Some(TaskPhase::AwaitingReview), None),
            "Code Review"
        );
        assert_eq!(
            kind_label(&document, Some(TaskPhase::AwaitingRuntimeQa), None),
            "Runtime QA"
        );
        assert_eq!(
            kind_label(&document, Some(TaskPhase::AwaitingSpecReconciliation), None),
            "Spec Reconciliation"
        );
        assert_eq!(
            kind_label(&document, Some(TaskPhase::PrPending), None),
            "Generate PR"
        );
        assert_eq!(
            next_step(TaskPhase::PrPending, None),
            "Prepare the parent pull request"
        );
        assert_eq!(
            kind_label(
                &document,
                Some(TaskPhase::Done),
                Some(crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization),
            ),
            "Generate PR"
        );
        assert_eq!(
            next_step(
                TaskPhase::Done,
                Some(crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization),
            ),
            "Review and authorize PR preparation"
        );
        let awaiting_auth =
            crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization;
        assert_eq!(board_column(Some(awaiting_auth), 4), 3);
        assert_eq!(pr_action_label(awaiting_auth), Some("Review PR"));
        let needs_attention = crate::core::lifecycle::PullRequestPreparationStatus::NeedsAttention;
        assert_eq!(board_column(Some(needs_attention), 4), 3);
        assert_eq!(pr_action_label(needs_attention), Some("Review PR failure"));
        assert_eq!(
            next_step(TaskPhase::Done, Some(needs_attention)),
            "Review PR failure in Task Details"
        );
    }

    #[test]
    fn completed_implementation_remains_done_while_pr_work_is_separate() {
        assert_eq!(next_step(TaskPhase::Done, None), "Implementation complete");
        assert_eq!(
            task_board_column(
                Some(TaskPhase::Done),
                Some(crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization),
                4,
            ),
            4
        );
        assert_eq!(
            task_board_column(
                Some(TaskPhase::Repairing),
                Some(crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization),
                2,
            ),
            3
        );
    }

    #[test]
    fn card_gate_track_keeps_missing_receipts_and_merge_state_unknown() {
        let task = lifecycle_task(TaskPhase::Done);
        assert_eq!(
            gate_track(Some(&task), None).as_deref(),
            Some(
                "Review not recorded · Runtime QA not recorded · Spec sync not recorded · PR not recorded · Merge not recorded"
            )
        );
        assert!(gate_track(None, None).is_none());
        assert!(
            gate_track(
                Some(&task),
                Some(crate::core::lifecycle::PullRequestPreparationStatus::Merged)
            )
            .unwrap()
            .contains("PR merged · Merge recorded")
        );
    }
}
