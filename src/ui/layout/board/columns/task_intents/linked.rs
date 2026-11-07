//! Derived delivery/review cards share the same immutable owner as work cards.
use super::*;

fn feature_owner<'a>(
    board: &'a crate::ui::planning_board::ViewModel,
    uid: &str,
) -> Option<&'a crate::core::planning_work::Work> {
    board.intent_for_feature(uid)
}

pub(in crate::ui::layout::board::columns) fn approval_owned(
    board: &crate::ui::planning_board::ViewModel,
    action: &crate::ui::feature_approval::Action,
) -> bool {
    action
        .plan_snapshot
        .as_ref()
        .and_then(|snapshot| feature_owner(board, &snapshot.feature_uid))
        .is_some()
}

pub(in crate::ui::layout::board::columns) fn review_owned(
    board: &crate::ui::planning_board::ViewModel,
    feature: &crate::core::lifecycle::FeatureGoal,
) -> bool {
    feature_owner(board, &feature.feature_uid).is_some()
}

pub(in crate::ui::layout::board::columns) fn clarification_owned(
    board: &crate::ui::planning_board::ViewModel,
    question: &crate::core::planning::QuestionV1,
) -> bool {
    board.question_owner(&question.feature_uid).is_some()
}

pub(super) fn collect<'a>(
    surface: &mut dyn Surface,
    board: &'a crate::ui::planning_board::ViewModel,
    column: usize,
    groups: &mut BTreeMap<String, Group<'a>>,
) {
    for question in &board.clarification_questions {
        if crate::ui::planning_board::question_column(question.status) != column
            || !board.question_visible(question)
        {
            continue;
        }
        let Some(root) = board.question_owner(&question.feature_uid) else {
            continue;
        };
        let group = groups.entry(root.uid.clone()).or_default();
        group.root = Some(root);
        group.clarifications.push(question);
    }
    for task in &board.lifecycle_pr_work {
        if cards::pr_task_column(task.preparation.status) != column {
            continue;
        }
        if let Some(root) = pr_owner(board, task) {
            if board.is_task_intent_archived(&root.uid) {
                continue;
            }
            let group = groups.entry(root.uid.clone()).or_default();
            group.root = Some(root);
            group.pr_work.push(task);
        }
    }
    if column == 3 {
        for action in surface
            .feature_actions(None)
            .into_iter()
            .filter(|action| !action.approved)
        {
            let Some(root) = action
                .plan_snapshot
                .as_ref()
                .and_then(|snapshot| feature_owner(board, &snapshot.feature_uid))
            else {
                continue;
            };
            if board.is_task_intent_archived(&root.uid) {
                continue;
            }
            let group = groups.entry(root.uid.clone()).or_default();
            group.root = Some(root);
            group.approvals.push(action);
        }
    }
    if column == 2 {
        for feature in &board.lifecycle_features {
            if feature.phase != crate::core::lifecycle::FeaturePhase::PlanReviewPending {
                continue;
            }
            let Some(root) = feature_owner(board, &feature.feature_uid) else {
                continue;
            };
            if board.is_task_intent_archived(&root.uid) {
                continue;
            }
            let group = groups.entry(root.uid.clone()).or_default();
            group.root = Some(root);
            group.plan_reviews.push(feature);
        }
    }
}
