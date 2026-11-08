use crate::core::lifecycle::TaskPhase;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FeatureDeliveryStage {
    #[default]
    Draft,
    Planned,
    Running,
    BuiltLocally,
    Verified,
    PullRequestOpen,
    Merged,
    Deployed,
    NeedsReview,
    NeedsAttention,
    Cancelled,
}

impl FeatureDeliveryStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Draft => "Draft",
            Self::Planned => "Planned",
            Self::Running => "Running",
            Self::BuiltLocally => "Built locally · awaiting checks",
            Self::Verified => "Verified locally",
            Self::PullRequestOpen => "Pull request open",
            Self::Merged => "Merged",
            Self::Deployed => "Deployed",
            Self::NeedsReview => "Needs review · drift detected",
            Self::NeedsAttention => "Needs attention",
            Self::Cancelled => "Cancelled",
        }
    }
}

pub fn derive_feature_delivery_stage(
    approved_plan: bool,
    task_phases: impl IntoIterator<Item = TaskPhase>,
    merged: bool,
    deployed: bool,
    needs_review: bool,
) -> Option<FeatureDeliveryStage> {
    let phases = task_phases.into_iter().collect::<Vec<_>>();
    if !approved_plan && phases.is_empty() {
        return Some(FeatureDeliveryStage::Draft);
    }
    if needs_review {
        return Some(FeatureDeliveryStage::NeedsReview);
    }
    if phases
        .iter()
        .any(|phase| matches!(phase, TaskPhase::Blocked | TaskPhase::Failed))
    {
        return Some(FeatureDeliveryStage::NeedsAttention);
    }
    if !phases.is_empty() && phases.iter().all(|phase| *phase == TaskPhase::Cancelled) {
        return Some(FeatureDeliveryStage::Cancelled);
    }
    if deployed {
        return Some(FeatureDeliveryStage::Deployed);
    }
    if merged || phases.contains(&TaskPhase::PrMerged) {
        return Some(FeatureDeliveryStage::Merged);
    }
    if phases.contains(&TaskPhase::PrOpened) {
        return Some(FeatureDeliveryStage::PullRequestOpen);
    }
    if phases.contains(&TaskPhase::PrPending) || phases.contains(&TaskPhase::Done) {
        return Some(FeatureDeliveryStage::Verified);
    }
    if phases.iter().any(|phase| {
        matches!(
            phase,
            TaskPhase::AwaitingReview
                | TaskPhase::AwaitingRuntimeQa
                | TaskPhase::AwaitingSpecReconciliation
        )
    }) {
        return Some(FeatureDeliveryStage::BuiltLocally);
    }
    if phases.iter().any(|phase| {
        matches!(
            phase,
            TaskPhase::Running | TaskPhase::Repairing | TaskPhase::ReReview
        )
    }) {
        return Some(FeatureDeliveryStage::Running);
    }
    approved_plan.then_some(FeatureDeliveryStage::Planned)
}

#[cfg(test)]
mod tests {
    use super::{FeatureDeliveryStage as Stage, derive_feature_delivery_stage as stage};
    use crate::core::lifecycle::TaskPhase as Task;

    #[test]
    fn stage_projection_distinguishes_the_feature_delivery_lifecycle() {
        let cases = [
            (false, vec![], false, false, false, Stage::Draft),
            (true, vec![Task::Ready], false, false, false, Stage::Planned),
            (
                true,
                vec![Task::Running],
                false,
                false,
                false,
                Stage::Running,
            ),
            (
                true,
                vec![Task::AwaitingRuntimeQa],
                false,
                false,
                false,
                Stage::BuiltLocally,
            ),
            (
                true,
                vec![Task::PrPending],
                false,
                false,
                false,
                Stage::Verified,
            ),
            (true, vec![Task::Done], false, false, false, Stage::Verified),
            (
                true,
                vec![Task::Blocked],
                false,
                false,
                false,
                Stage::NeedsAttention,
            ),
            (
                true,
                vec![Task::Failed],
                false,
                false,
                false,
                Stage::NeedsAttention,
            ),
            (
                true,
                vec![Task::Cancelled],
                false,
                false,
                false,
                Stage::Cancelled,
            ),
            (
                true,
                vec![Task::PrOpened],
                false,
                false,
                false,
                Stage::PullRequestOpen,
            ),
            (true, vec![], true, false, false, Stage::Merged),
            (true, vec![], true, true, false, Stage::Deployed),
            (
                true,
                vec![Task::PrMerged],
                true,
                false,
                true,
                Stage::NeedsReview,
            ),
        ];
        for (approved, tasks, merged, deployed, review, expected) in cases {
            assert_eq!(
                stage(approved, tasks, merged, deployed, review),
                Some(expected)
            );
        }
        assert_eq!(stage(false, [], false, false, false), Some(Stage::Draft));
        assert_eq!(stage(false, [Task::Ready], false, false, false), None);
    }

    #[test]
    fn status_labels_are_plain_language_and_separate_merge_from_deployment() {
        assert_eq!(Stage::PullRequestOpen.label(), "Pull request open");
        assert_eq!(Stage::Merged.label(), "Merged");
        assert_eq!(Stage::Deployed.label(), "Deployed");
        assert_eq!(Stage::NeedsReview.label(), "Needs review · drift detected");
        assert_eq!(Stage::NeedsAttention.label(), "Needs attention");
        assert_eq!(Stage::Cancelled.label(), "Cancelled");
        assert_ne!(Stage::Merged.label(), Stage::Deployed.label());
    }
}
