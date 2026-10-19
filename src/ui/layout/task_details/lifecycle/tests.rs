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

/// Production UI boundary: renders lifecycle relationships and release status.
/// A model-only assertion cannot prove task links and merged state are visible.
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
        parent_task: None,
        dependencies: vec![crate::ui::task_detail::TaskRelationView {
            title: "Foundation task".into(),
            status: "Done".into(),
            acceptance_criteria: Vec::new(),
        }],
        subtasks: vec![crate::ui::task_detail::TaskRelationView {
            title: "Reviewed child task".into(),
            status: "Awaiting approval".into(),
            acceptance_criteria: vec!["The child behavior is observable.".into()],
        }],
        task_revision: 4,
        code_review: None,
        runtime_qa: None,
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
    assert!(rendered.iter().any(|(text, _)| text == "Dependencies"));
    assert!(
        rendered
            .iter()
            .any(|(text, _)| text.contains("Foundation task · Done"))
    );
    assert!(
        rendered
            .iter()
            .any(|(text, _)| text.contains("Reviewed child task · Awaiting approval"))
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
