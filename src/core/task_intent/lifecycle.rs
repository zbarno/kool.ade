use super::TaskIntentLifecycle;
use crate::core::{
    lifecycle::{CurrentImplementationEvidenceV1, DeploymentEvidenceV1, FeaturePhase, TaskPhase},
    planning_work::{WorkKind, WorkStatus},
};

pub(super) fn derive_lifecycle(
    kind: WorkKind,
    status: WorkStatus,
    children: &[WorkStatus],
    feature: Option<FeaturePhase>,
    tasks: &[TaskPhase],
    deployment: Option<(&DeploymentEvidenceV1, &CurrentImplementationEvidenceV1)>,
    cancelled: bool,
) -> TaskIntentLifecycle {
    // Only root/feature cancellation cancels the whole intent. A cancelled
    // executable child is a terminal child outcome and must be aggregated
    // with its siblings below.
    if cancelled || feature == Some(FeaturePhase::Cancelled) {
        return TaskIntentLifecycle::Cancelled;
    }
    if status == WorkStatus::NeedsAttention || children.contains(&WorkStatus::NeedsAttention) {
        return TaskIntentLifecycle::NeedsAttention;
    }
    if tasks.contains(&TaskPhase::Blocked) {
        return TaskIntentLifecycle::NeedsAttention;
    }

    let unfinished_task = tasks.iter().any(|phase| {
        matches!(
            phase,
            TaskPhase::Ready
                | TaskPhase::Running
                | TaskPhase::AwaitingReview
                | TaskPhase::Repairing
                | TaskPhase::ReReview
                | TaskPhase::AwaitingRuntimeQa
                | TaskPhase::AwaitingSpecReconciliation
        )
    });
    let unfinished_child = children.iter().any(|status| {
        matches!(
            status,
            WorkStatus::Todo | WorkStatus::InProgress | WorkStatus::InReview
        )
    });
    let terminal_non_success = tasks
        .iter()
        .any(|phase| matches!(phase, TaskPhase::Failed | TaskPhase::Cancelled));
    let terminal_success = children.contains(&WorkStatus::Done)
        || tasks.contains(&TaskPhase::Done)
        || tasks.iter().any(|phase| {
            matches!(
                phase,
                TaskPhase::PrPending | TaskPhase::PrOpened | TaskPhase::PrMerged
            )
        });

    if terminal_non_success && (unfinished_task || unfinished_child) {
        return TaskIntentLifecycle::InProgress;
    }
    if terminal_non_success && terminal_success {
        return TaskIntentLifecycle::PartiallyComplete;
    }
    if tasks.contains(&TaskPhase::Failed) {
        return TaskIntentLifecycle::Failed;
    }
    if tasks.contains(&TaskPhase::Cancelled) {
        return TaskIntentLifecycle::Cancelled;
    }
    if children.contains(&WorkStatus::Done)
        && children.iter().any(|child| *child != WorkStatus::Done)
    {
        return TaskIntentLifecycle::PartiallyComplete;
    }

    // An earlier phase in any parallel child prevents the root from claiming
    // that publication has completed for the whole request.
    if unfinished_task
        || unfinished_child
        || status == WorkStatus::InProgress
        || status == WorkStatus::Todo
        || status == WorkStatus::InReview
        || children.contains(&WorkStatus::Todo)
        || children.contains(&WorkStatus::InProgress)
        || children.contains(&WorkStatus::InReview)
    {
        return if tasks.iter().any(|phase| {
            matches!(
                phase,
                TaskPhase::AwaitingReview
                    | TaskPhase::ReReview
                    | TaskPhase::AwaitingRuntimeQa
                    | TaskPhase::AwaitingSpecReconciliation
            )
        }) || status == WorkStatus::InReview
            || children.contains(&WorkStatus::InReview)
        {
            TaskIntentLifecycle::AwaitingReview
        } else {
            TaskIntentLifecycle::InProgress
        };
    }
    if tasks.contains(&TaskPhase::PrPending) {
        return TaskIntentLifecycle::PreparingPullRequest;
    }
    if tasks.contains(&TaskPhase::PrOpened) {
        return TaskIntentLifecycle::PullRequestOpen;
    }
    if tasks.contains(&TaskPhase::PrMerged) {
        if deployment
            .is_some_and(|(receipt, current)| receipt.is_successful_production_for_current(current))
        {
            return TaskIntentLifecycle::Deployed;
        }
        return TaskIntentLifecycle::Merged;
    }

    if matches!(kind, WorkKind::Feature | WorkKind::Bug) {
        return match feature {
            Some(FeaturePhase::Complete) if tasks.contains(&TaskPhase::Done) => {
                TaskIntentLifecycle::Verified
            }
            Some(FeaturePhase::Complete) => TaskIntentLifecycle::Unknown,
            Some(FeaturePhase::Cancelled) => TaskIntentLifecycle::Cancelled,
            Some(FeaturePhase::AwaitingAnswers | FeaturePhase::AwaitingPlanConfirmation)
            | Some(FeaturePhase::PlanReviewPending | FeaturePhase::PlanRework) => {
                TaskIntentLifecycle::AwaitingReview
            }
            Some(FeaturePhase::Executing | FeaturePhase::Decomposing) => {
                TaskIntentLifecycle::InProgress
            }
            Some(FeaturePhase::Exploring | FeaturePhase::ReadyToDecompose) => {
                TaskIntentLifecycle::Planning
            }
            None if status == WorkStatus::Done => TaskIntentLifecycle::Unknown,
            None => TaskIntentLifecycle::Planning,
        };
    }
    if tasks.contains(&TaskPhase::Done) || status == WorkStatus::Done && tasks.is_empty() {
        return TaskIntentLifecycle::Complete;
    }
    if status == WorkStatus::InReview || children.contains(&WorkStatus::InReview) {
        return TaskIntentLifecycle::AwaitingReview;
    }
    if status == WorkStatus::Todo && children.is_empty() && tasks.is_empty() {
        return TaskIntentLifecycle::Planning;
    }
    if tasks.contains(&TaskPhase::Ready) || status == WorkStatus::Todo {
        return TaskIntentLifecycle::InProgress;
    }
    if children.contains(&WorkStatus::Done) {
        return TaskIntentLifecycle::PartiallyComplete;
    }
    TaskIntentLifecycle::Unknown
}
