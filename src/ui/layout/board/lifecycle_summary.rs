use crate::{
    core::lifecycle::{FeatureGoal, FeaturePhase, TaskPhase, TaskRecord},
    ui::planning_board::ViewModel,
};

pub(super) fn paint(ui: &mut egui::Ui, board: &ViewModel) {
    if board.lifecycle_features.is_empty() {
        return;
    }
    egui::CollapsingHeader::new("Feature lifecycle")
        .default_open(false)
        .show(ui, |ui| {
            for feature in &board.lifecycle_features {
                let tasks = board
                    .lifecycle_feature_tasks
                    .iter()
                    .filter(|task| task.feature_uid == feature.feature_uid)
                    .collect::<Vec<_>>();
                paint_feature(ui, feature, &tasks);
            }
        });
}

fn paint_feature(ui: &mut egui::Ui, feature: &FeatureGoal, tasks: &[&TaskRecord]) {
    let title = feature
        .draft
        .lines()
        .find(|line| line.starts_with('#'))
        .map(|line| line.trim_start_matches('#').trim())
        .filter(|title| !title.is_empty())
        .unwrap_or(&feature.original_intent);
    ui.group(|ui| {
        ui.label(egui::RichText::new(title).strong());
        let phases = tasks.iter().map(|task| task.phase).collect::<Vec<_>>();
        let pr_work_count = tasks
            .iter()
            .filter(|task| task.pr_work_item_uid.is_some())
            .count();
        let counts = stage_counts(feature, &phases, pr_work_count);
        ui.horizontal_wrapped(|ui| {
            for (label, count) in counts {
                if count > 0 {
                    crate::ui::theme::badge(
                        ui,
                        &format!("{label} · {count}"),
                        crate::ui::theme::PANEL,
                        crate::ui::theme::TEXT_DIM,
                    );
                }
            }
            let blockers = tasks
                .iter()
                .filter(|task| matches!(task.phase, TaskPhase::Blocked | TaskPhase::Failed))
                .count();
            if blockers > 0 || !feature.unresolved_questions.is_empty() {
                crate::ui::theme::badge(
                    ui,
                    &format!(
                        "Blockers · {}",
                        blockers + feature.unresolved_questions.len()
                    ),
                    crate::ui::theme::WARNING.gamma_multiply(0.14),
                    crate::ui::theme::WARNING,
                );
            }
        });
    });
}

fn stage_counts(
    feature: &FeatureGoal,
    task_phases: &[TaskPhase],
    pr_work_count: usize,
) -> Vec<(&'static str, usize)> {
    let count = |phases: &[TaskPhase]| {
        task_phases
            .iter()
            .filter(|phase| phases.contains(phase))
            .count()
    };
    let pr_phase_count = count(&[
        TaskPhase::PrPending,
        TaskPhase::PrOpened,
        TaskPhase::PrMerged,
    ]);
    let mut stages = vec![
        (
            "Questioning",
            usize::from(matches!(
                feature.phase,
                FeaturePhase::Exploring | FeaturePhase::AwaitingAnswers
            )),
        ),
        (
            "Awaiting Plan Confirmation",
            usize::from(feature.phase == FeaturePhase::AwaitingPlanConfirmation),
        ),
        (
            "Independent Plan Review",
            usize::from(feature.phase == FeaturePhase::PlanReviewPending),
        ),
        (
            "Plan Rework",
            usize::from(feature.phase == FeaturePhase::PlanRework),
        ),
        (
            "Task Generation",
            usize::from(matches!(
                feature.phase,
                FeaturePhase::ReadyToDecompose | FeaturePhase::Decomposing
            )),
        ),
        (
            "Implementation",
            count(&[TaskPhase::Ready, TaskPhase::Running])
                + count(&[TaskPhase::Done]).saturating_sub(pr_work_count),
        ),
        (
            "QA & Review",
            count(&[
                TaskPhase::AwaitingReview,
                TaskPhase::Repairing,
                TaskPhase::ReReview,
                TaskPhase::AwaitingRuntimeQa,
            ]),
        ),
        ("Spec Sync", count(&[TaskPhase::AwaitingSpecReconciliation])),
        ("PR Work", pr_phase_count.max(pr_work_count)),
    ];
    if matches!(
        feature.phase,
        FeaturePhase::Complete | FeaturePhase::Cancelled
    ) {
        stages.push((
            if feature.phase == FeaturePhase::Complete {
                "Complete"
            } else {
                "Cancelled"
            },
            1,
        ));
    }
    stages
}

#[cfg(test)]
mod tests {
    use super::stage_counts;
    use crate::core::lifecycle::{FeatureGoal, FeaturePhase, TaskPhase};

    #[test]
    fn feature_summary_groups_review_qa_spec_and_pr_stages() {
        let feature = FeatureGoal {
            schema_version: 1,
            feature_uid: "feature".into(),
            original_intent: "intent".into(),
            draft: String::new(),
            scope: vec![],
            non_goals: vec![],
            revision: 1,
            phase: FeaturePhase::Executing,
            approved_plan_sha256: Some("hash".into()),
            plan_snapshot_uid: None,
            plan_snapshot_sha256: None,
            unresolved_questions: vec![],
            decision_refs: vec![],
            review_refs: vec![],
            readiness_evidence_refs: vec![],
            task_uids: vec![],
        };
        let counts = stage_counts(
            &feature,
            &[
                TaskPhase::AwaitingReview,
                TaskPhase::AwaitingRuntimeQa,
                TaskPhase::AwaitingSpecReconciliation,
                TaskPhase::Done,
                TaskPhase::PrOpened,
            ],
            1,
        );
        assert_eq!(
            counts.iter().find(|(label, _)| *label == "QA & Review"),
            Some(&("QA & Review", 2))
        );
        assert_eq!(
            counts.iter().find(|(label, _)| *label == "Spec Sync"),
            Some(&("Spec Sync", 1))
        );
        assert_eq!(
            counts.iter().find(|(label, _)| *label == "PR Work"),
            Some(&("PR Work", 1))
        );
        assert_eq!(
            counts.iter().find(|(label, _)| *label == "Implementation"),
            Some(&("Implementation", 0))
        );
        let mut rework = feature;
        rework.phase = FeaturePhase::PlanRework;
        assert_eq!(
            stage_counts(&rework, &[], 0)
                .iter()
                .find(|(label, _)| *label == "Plan Rework"),
            Some(&("Plan Rework", 1))
        );
    }
}
