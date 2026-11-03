//! TaskIntent projection and type-aware outcome gates.
//!
//! A root Work record is the durable identity and original request. This
//! projection aggregates only typed state supplied by its owners; it does not
//! persist a second root status or infer success from child counts.
use crate::core::{
    lifecycle::{FeaturePhase, TaskPhase},
    planning_work::{Work, WorkKind, WorkStatus},
};
use serde::{Deserialize, Serialize};

mod lifecycle;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskIntentLifecycle {
    Planning,
    InProgress,
    AwaitingReview,
    NeedsAttention,
    PartiallyComplete,
    Verified,
    PreparingPullRequest,
    PullRequestOpen,
    Merged,
    Deployed,
    Complete,
    Cancelled,
    Failed,
    Unknown,
}

impl TaskIntentLifecycle {
    pub fn label(self) -> &'static str {
        match self {
            Self::Planning => "Planning",
            Self::InProgress => "In progress",
            Self::AwaitingReview => "Awaiting review",
            Self::NeedsAttention => "Needs attention",
            Self::PartiallyComplete => "Partially complete",
            Self::Verified => "Implementation verified",
            Self::PreparingPullRequest => "Preparing pull request",
            Self::PullRequestOpen => "Pull request open",
            Self::Merged => "Merged",
            Self::Deployed => "Deployed",
            Self::Complete => "Complete",
            Self::Cancelled => "Cancelled",
            Self::Failed => "Failed",
            Self::Unknown => "Outcome unknown",
        }
    }

    pub fn next_action(self) -> &'static str {
        match self {
            Self::Planning => "Continue planning and confirm the approved outcome.",
            Self::InProgress => "Continue the work and check its recorded outcome.",
            Self::AwaitingReview => "Complete the required review or verification step.",
            Self::NeedsAttention => "Resolve the blocker before treating this request as complete.",
            Self::PartiallyComplete => "Review the unfinished or failed child work.",
            Self::Verified => "Publication has not been recorded as merged.",
            Self::PreparingPullRequest => "Finish the authorized pull request preparation.",
            Self::PullRequestOpen => "Review and merge the open pull request when approved.",
            Self::Merged => "The pull request is merged; deployment is a separate outcome.",
            Self::Deployed => "The requested outcome is deployed.",
            Self::Complete => "The requested planning or answer outcome is recorded.",
            Self::Cancelled => "The request was cancelled; prior work remains in its history.",
            Self::Failed => "Review the failure and decide whether to retry or close the request.",
            Self::Unknown => "Refresh the relevant records before claiming an outcome.",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskIntent<'a> {
    pub uid: &'a str,
    pub request_type: WorkKind,
    pub original_goal: &'a str,
    pub conversation_key: &'a str,
    pub lifecycle: TaskIntentLifecycle,
}

impl<'a> TaskIntent<'a> {
    pub fn project(
        root: &'a Work,
        child_work: &[WorkStatus],
        feature_phase: Option<FeaturePhase>,
        task_phases: &[TaskPhase],
        deployment: Option<(
            &crate::core::lifecycle::DeploymentEvidenceV1,
            &crate::core::lifecycle::CurrentImplementationEvidenceV1,
        )>,
    ) -> Option<Self> {
        Self::project_with_cancelled(
            root,
            child_work,
            feature_phase,
            task_phases,
            deployment,
            false,
        )
    }

    pub fn project_with_cancelled(
        root: &'a Work,
        child_work: &[WorkStatus],
        feature_phase: Option<FeaturePhase>,
        task_phases: &[TaskPhase],
        deployment: Option<(
            &crate::core::lifecycle::DeploymentEvidenceV1,
            &crate::core::lifecycle::CurrentImplementationEvidenceV1,
        )>,
        cancelled: bool,
    ) -> Option<Self> {
        if root.parent_uid.is_some() || root.task_intent_root_uid() != Some(root.uid.as_str()) {
            return None;
        }
        Some(Self {
            uid: &root.uid,
            request_type: root.kind,
            original_goal: root.original_goal.as_deref().unwrap_or(&root.request),
            conversation_key: &root.key,
            lifecycle: lifecycle::derive_lifecycle(
                root.kind,
                root.status,
                child_work,
                feature_phase,
                task_phases,
                deployment,
                cancelled,
            ),
        })
    }

    /// Include linked human decision records in the aggregate. Open items are
    /// unresolved work; resolved items remain visible but no longer block the
    /// intent's outcome.
    pub fn project_with_decisions(
        root: &'a Work,
        child_work: &[WorkStatus],
        decisions: &[crate::domain::item::ItemStatus],
        feature_phase: Option<FeaturePhase>,
        task_phases: &[TaskPhase],
        deployment: Option<(
            &crate::core::lifecycle::DeploymentEvidenceV1,
            &crate::core::lifecycle::CurrentImplementationEvidenceV1,
        )>,
        cancelled: bool,
    ) -> Option<Self> {
        let mut statuses = child_work.to_vec();
        statuses.extend(decisions.iter().map(|status| match status {
            crate::domain::item::ItemStatus::Open => WorkStatus::NeedsAttention,
            crate::domain::item::ItemStatus::Resolved => WorkStatus::Done,
        }));
        Self::project_with_cancelled(
            root,
            &statuses,
            feature_phase,
            task_phases,
            deployment,
            cancelled,
        )
    }
}

#[cfg(test)]
mod tests;
