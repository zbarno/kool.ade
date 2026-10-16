use super::*;

pub(super) fn paint(
    ui: &mut egui::Ui,
    lifecycle: Option<&crate::ui::task_detail::LifecycleView>,
    error: Option<&str>,
) {
    if let Some(error) = error {
        ui.group(|ui| {
            ui.label(theme::section_heading("Lifecycle status unavailable"));
            ui.colored_label(theme::WARNING, error);
        });
        return;
    }
    let Some(lifecycle) = lifecycle else {
        return;
    };
    ui.group(|ui| {
        ui.label(theme::section_heading("Lifecycle"));
        ui.label(format!(
            "Feature · {}",
            feature_phase_label(lifecycle.feature_phase)
        ));
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "Task · {} · revision {}",
                crate::ui::task_detail::task_phase_label(lifecycle.task_phase),
                lifecycle.task_revision
            ));
        });
        ui.label("Recorded gates");
        receipt_row(ui, "Code review", lifecycle.code_review.as_ref());
        receipt_row(ui, "Runtime QA", lifecycle.runtime_qa.as_ref());
        receipt_row(
            ui,
            "Specification reconciliation",
            lifecycle.spec_reconciliation.as_ref(),
        );
        ui.label(theme::helper_text(
            "Read-only lifecycle receipts. Missing evidence is shown as not recorded.",
        ));
    });
}

fn receipt_row(
    ui: &mut egui::Ui,
    label: &str,
    receipt: Option<&crate::core::lifecycle::GateReceipt>,
) {
    let text = receipt_summary(receipt);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(label).strong());
        ui.label(text);
    });
}

fn receipt_summary(receipt: Option<&crate::core::lifecycle::GateReceipt>) -> String {
    receipt.map_or_else(
        || "Not recorded".to_owned(),
        |receipt| {
            format!(
                "{} · {} evidence item(s)",
                decision_label(receipt.decision),
                receipt.evidence_refs.len()
            )
        },
    )
}

fn decision_label(decision: crate::core::lifecycle::GateDecision) -> &'static str {
    use crate::core::lifecycle::GateDecision;
    match decision {
        GateDecision::Approved => "Approved",
        GateDecision::Rejected => "Rejected",
        GateDecision::ChangesRequested => "Changes requested",
        GateDecision::NotApplicable => "Not applicable",
    }
}

fn feature_phase_label(phase: crate::core::lifecycle::FeaturePhase) -> &'static str {
    use crate::core::lifecycle::FeaturePhase;
    match phase {
        FeaturePhase::Exploring => "Exploring",
        FeaturePhase::AwaitingAnswers => "Awaiting answers",
        FeaturePhase::AwaitingPlanConfirmation => "Awaiting plan confirmation",
        FeaturePhase::PlanReviewPending => "Independent plan review",
        FeaturePhase::PlanRework => "Plan rework",
        FeaturePhase::ReadyToDecompose => "Ready to generate tasks",
        FeaturePhase::Decomposing => "Generating tasks",
        FeaturePhase::Executing => "In implementation",
        FeaturePhase::Complete => "Complete",
        FeaturePhase::Cancelled => "Cancelled",
    }
}

#[cfg(test)]
mod tests {
    use super::{decision_label, feature_phase_label, receipt_summary};
    use crate::core::lifecycle::{FeaturePhase, GateDecision, GateReceipt, TaskPhase};

    fn mocked_receipt(decision: GateDecision) -> GateReceipt {
        GateReceipt {
            receipt_uid: String::new(),
            subject_uid: String::new(),
            subject_sha256: String::new(),
            actor: crate::core::lifecycle::GateActor::IndependentReviewer,
            decision,
            recorded_at_unix_ms: 0,
            evidence_refs: vec!["test:evidence".into()],
            prompt_sha256: None,
            prompt_manifest: None,
            reviewer_run_id: None,
            author_run_id: None,
            reviewer_session_id: None,
            author_session_id: None,
        }
    }

    #[test]
    fn mocked_review_receipts_preserve_review_outcomes() {
        assert_eq!(
            decision_label(mocked_receipt(GateDecision::Approved).decision),
            "Approved"
        );
        assert_eq!(
            decision_label(mocked_receipt(GateDecision::ChangesRequested).decision),
            "Changes requested"
        );
    }

    #[test]
    fn missing_receipts_are_not_a_pass() {
        assert_eq!(receipt_summary(None), "Not recorded");
    }

    #[test]
    fn lifecycle_phase_labels_keep_review_and_release_states_distinct() {
        assert_eq!(
            crate::ui::task_detail::task_phase_label(TaskPhase::AwaitingReview),
            "Awaiting review"
        );
        assert_eq!(
            crate::ui::task_detail::task_phase_label(TaskPhase::PrMerged),
            "PR merged"
        );
        assert_eq!(
            feature_phase_label(FeaturePhase::AwaitingPlanConfirmation),
            "Awaiting plan confirmation"
        );
    }

    #[test]
    fn mocked_review_receipt_displays_outcome_and_evidence_count() {
        let receipt = mocked_receipt(GateDecision::ChangesRequested);
        assert_eq!(
            receipt_summary(Some(&receipt)),
            "Changes requested · 1 evidence item(s)"
        );
    }
}
