use super::*;
#[path = "lifecycle/code_review.rs"]
mod code_review;
mod qa_gallery;
mod references;

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut Option<&mut dyn crate::ui::Surface>,
    ticket: &str,
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
        references::relationships(ui, surface, lifecycle);
        ui.label("Specification state");
        state_row(
            ui,
            "Current rendered product-spec hash",
            lifecycle.current_spec_hash.as_deref(),
        );
        state_row(
            ui,
            "Implemented on branch",
            lifecycle.implementation_head_sha.as_deref(),
        );
        state_row(ui, "Target planned", lifecycle.approved_target_hash.as_deref());
        state_row(
            ui,
            "Merged commit",
            lifecycle.merged_commit_sha.as_deref(),
        );
        state_row(ui, "Deployed", lifecycle.deployed_spec_hash.as_deref());
        state_row(ui, "Latest production deployment status", lifecycle.deployment_status.as_deref());
        if let Some(observed_at) = lifecycle.deployment_observed_at_ms {
            let observed = chrono::DateTime::from_timestamp_millis(observed_at as i64)
                .map_or_else(|| observed_at.to_string(), |time| time.to_rfc3339());
            ui.small(format!("Deployment status observed · {observed}"));
        }
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
        references::deltas(ui, surface, lifecycle);
        ui.label("Recorded gates");
        references::receipts(ui, surface, lifecycle);
        qa_gallery::paint(ui, surface, ticket, lifecycle);
        code_review::paint(ui, surface, lifecycle);
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
    use super::references::{decision_label, receipt_summary};
    use super::{feature_phase_label, paint};
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

    /// Production UI boundary: renders lifecycle relationships and release status.
    /// A model-only assertion cannot prove task links and merged state are visible.
    /// Uses synthetic in-memory data only, no external services, negligible setup.
    #[test]
    fn merged_pr_displays_deployed_status_as_not_recorded() {
        use crate::core::lifecycle::{
            FeaturePhase, PullRequestPreparationStatus, SpecReconciliationStatusV1, TaskPhase,
        };
        let lifecycle = crate::ui::task_detail::LifecycleView {
            feature_uid: "feature-uid".into(),
            feature_phase: FeaturePhase::Complete,
            task_phase: TaskPhase::PrMerged,
            parent_task_uid: None,
            parent_task: None,
            dependencies: vec![crate::ui::task_detail::TaskRelationView {
                planning_item_uid: "dependency-uid".into(),
                lifecycle_task_uid: Some("dependency-task-uid".into()),
                title: "Foundation task".into(),
                status: "Done".into(),
                acceptance_criteria: Vec::new(),
            }],
            subtasks: vec![crate::ui::task_detail::TaskRelationView {
                planning_item_uid: "subtask-uid".into(),
                lifecycle_task_uid: Some("subtask-task-uid".into()),
                title: "Reviewed child task".into(),
                status: "Awaiting approval".into(),
                acceptance_criteria: vec!["The child behavior is observable.".into()],
            }],
            task_revision: 4,
            code_review: None,
            code_review_attempts: Vec::new(),
            current_code_review: None,
            code_review_dispositions: Vec::new(),
            code_review_load_error: None,
            runtime_qa: None,
            runtime_qa_run_id: None,
            spec_reconciliation: None,
            current_spec_hash: Some("current-hash".into()),
            implementation_head_sha: Some("reviewed-head".into()),
            approved_target_hash: Some("approved-hash".into()),
            deployed_spec_hash: None,
            deployment_status: None,
            deployment_observed_at_ms: None,
            spec_revision: Some(5),
            spec_status: Some(SpecReconciliationStatusV1::Synchronized),
            pending_spec_triggers: 0,
            specification_deltas: Vec::new(),
            pull_request_status: Some(PullRequestPreparationStatus::Merged),
            merged_commit_sha: Some("provider-merge-sha".into()),
            runtime_qa_evidence: None,
            runtime_qa_evidence_error: None,
            runtime_qa_applicability: None,
        };
        let context = egui::Context::default();
        let mut surface = None;
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
                    paint(ui, &mut surface, "task.md", Some(&lifecycle), None);
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
                .any(|(text, _)| text.contains("Merged commit"))
        );
        assert!(rendered.iter().any(|(text, _)| text == "Dependencies"));
        assert!(
            rendered
                .iter()
                .any(|(text, _)| text.contains("Dependency: dependency-task-uid"))
        );
        assert!(
            rendered
                .iter()
                .any(|(text, _)| text.contains("Subtask: subtask-task-uid"))
        );
        assert!(rendered.iter().any(|(text, _)| text == "Foundation task"));
        assert!(
            rendered
                .iter()
                .any(|(text, _)| text == "Reviewed child task")
        );
        assert!(
            rendered
                .iter()
                .any(|(text, _)| { text.contains("The child behavior is observable.") })
        );
        assert!(
            rendered
                .iter()
                .any(|(text, _)| text.contains("this parent's single pull request"))
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
