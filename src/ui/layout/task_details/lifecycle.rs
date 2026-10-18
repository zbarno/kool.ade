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
        ui.label("Specification state");
        state_row(
            ui,
            "Current rendered product-spec hash",
            lifecycle.current_spec_hash.as_deref(),
        );
        state_row(
            ui,
            "Reviewed implementation",
            lifecycle.implementation_head_sha.as_deref(),
        );
        state_row(ui, "Approved target", lifecycle.approved_target_hash.as_deref());
        state_row(
            ui,
            "Provider-reported merge commit",
            lifecycle.merged_commit_sha.as_deref(),
        );
        state_row(ui, "Deployed", lifecycle.deployed_spec_hash.as_deref());
        ui.label(format!(
            "Pull request status · {}",
            lifecycle.pull_request_status.map_or("not recorded", |status| match status {
                crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization => "awaiting authorization",
                crate::core::lifecycle::PullRequestPreparationStatus::Publishing => "publishing",
                crate::core::lifecycle::PullRequestPreparationStatus::NeedsAttention => "needs attention",
                crate::core::lifecycle::PullRequestPreparationStatus::Opened => "open",
                crate::core::lifecycle::PullRequestPreparationStatus::ClosedUnmerged => "closed without merge",
                crate::core::lifecycle::PullRequestPreparationStatus::Merged => "merged",
            })
        ));
        ui.label(format!(
            "Revision {} · reconciliation {}",
            lifecycle.spec_revision.map_or_else(|| "not recorded".to_owned(), |seq| seq.to_string()),
            lifecycle.spec_status.map_or("not recorded", |status| match status {
                crate::core::lifecycle::SpecReconciliationStatusV1::Synchronized => "synchronized",
                crate::core::lifecycle::SpecReconciliationStatusV1::OutstandingNonblockingFindings => "open findings",
                crate::core::lifecycle::SpecReconciliationStatusV1::BlockingDrift => "blocking drift",
                crate::core::lifecycle::SpecReconciliationStatusV1::NeedsHumanDecision => "needs decision",
            })
        ));
        ui.label(format!("Pending specification triggers · {}", lifecycle.pending_spec_triggers));
        if !lifecycle.specification_deltas.is_empty() {
            ui.label("Specification findings linked to this task");
            for (revision, delta) in &lifecycle.specification_deltas {
                ui.label(format!(
                    "Revision {revision} · {} · {:?}",
                    delta.requirement_id, delta.classification
                ));
                if let Some(proposed) = delta.proposed_target.as_deref() {
                    ui.small(format!("Proposed target: {proposed}"));
                }
                if let Some(observed) = delta.observed_current.as_deref() {
                    ui.small(format!("Observed current state: {observed}"));
                }
                for evidence in &delta.evidence_refs {
                    ui.small(format!("Evidence: {evidence}"));
                }
            }
        }
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

fn state_row(ui: &mut egui::Ui, label: &str, hash: Option<&str>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(label).strong());
        ui.label(hash.unwrap_or("Not recorded"));
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
    use super::{decision_label, feature_phase_label, paint, receipt_summary};
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

    /// Production UI boundary: renders the lifecycle view consumed by task details.
    /// A model-only assertion cannot prove the merged state is labeled honestly.
    /// Uses synthetic in-memory data only, no external services, negligible setup.
    #[test]
    fn merged_pr_displays_deployed_status_as_not_recorded() {
        use crate::core::lifecycle::{
            FeaturePhase, PullRequestPreparationStatus, SpecReconciliationStatusV1, TaskPhase,
        };
        let lifecycle = crate::ui::task_detail::LifecycleView {
            feature_phase: FeaturePhase::Complete,
            task_phase: TaskPhase::PrMerged,
            parent_task_uid: None,
            task_revision: 4,
            code_review: None,
            runtime_qa: None,
            spec_reconciliation: None,
            current_spec_hash: Some("current-hash".into()),
            implementation_head_sha: Some("reviewed-head".into()),
            approved_target_hash: Some("approved-hash".into()),
            deployed_spec_hash: None,
            spec_revision: Some(5),
            spec_status: Some(SpecReconciliationStatusV1::Synchronized),
            pending_spec_triggers: 0,
            specification_deltas: Vec::new(),
            pull_request_status: Some(PullRequestPreparationStatus::Merged),
            merged_commit_sha: Some("provider-merge-sha".into()),
        };
        let context = egui::Context::default();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    paint(ui, Some(&lifecycle), None);
                });
            },
        );
        output.textures_delta.clear();
        let rendered = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((text.galley.text().to_owned(), text.pos)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(
            rendered
                .windows(2)
                .any(|row| row[0].0 == "Deployed" && row[1].0 == "Not recorded"),
            "deployed status must be shown as not recorded; rendered={rendered:?}"
        );
        assert!(
            rendered
                .iter()
                .any(|(text, _)| text.contains("Pull request status · merged"))
        );
        assert!(
            rendered
                .iter()
                .any(|(text, _)| text.contains("Provider-reported merge commit"))
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
