use super::*;

#[test]
fn done_planning_children_do_not_satisfy_approval_qa_or_publication_gates() {
    let feature = root(WorkKind::Feature, WorkStatus::Done);
    let done_children = [WorkStatus::Done, WorkStatus::Done];
    for (feature_phase, task_phases, expected, action_fragment) in [
        (
            Some(FeaturePhase::AwaitingPlanConfirmation),
            vec![],
            TaskIntentLifecycle::AwaitingReview,
            "review",
        ),
        (
            Some(FeaturePhase::Complete),
            vec![TaskPhase::AwaitingRuntimeQa],
            TaskIntentLifecycle::AwaitingReview,
            "verification",
        ),
        (
            Some(FeaturePhase::Complete),
            vec![TaskPhase::Done],
            TaskIntentLifecycle::Verified,
            "Publication",
        ),
    ] {
        let intent =
            TaskIntent::project(&feature, &done_children, feature_phase, &task_phases, None)
                .unwrap();
        assert_eq!(intent.lifecycle, expected);
        assert_ne!(intent.lifecycle, TaskIntentLifecycle::Complete);
        assert!(intent.lifecycle.next_action().contains(action_fragment));
    }
}
