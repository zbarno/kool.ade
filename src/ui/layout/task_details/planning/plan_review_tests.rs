use crate::ui::task_detail::LifecyclePlanReviewView;

/// Production presentation boundary: the reviewer card must expose the exact
/// confirmed snapshot and indicate a pending independent review. A data-only
/// assertion cannot verify the Task Details copy; this egui render test uses
/// synthetic in-memory evidence and has no external dependencies.
#[test]
fn plan_review_details_show_confirmed_snapshot_and_pending_reviewer() {
    let snapshot = crate::core::lifecycle::PlanSnapshotV1 {
        schema_version: 1,
        uid: "snapshot-uid".into(),
        feature_uid: "feature-uid".into(),
        feature_id: "feature-123".into(),
        original_goal_ref: "goal-1".into(),
        original_goal: String::new(),
        intent: "Add a safe task review view".into(),
        outcome: "Users can see verified plan status".into(),
        current_state: "Plan status is hidden".into(),
        target_behavior: "Show the review receipt".into(),
        scope: vec!["Display the confirmed snapshot".into()],
        non_goals: vec!["Do not add another approval flow".into()],
        requirements: Vec::new(),
        acceptance_criteria: vec!["The receipt is visible".into()],
        architecture: "Use persisted lifecycle state".into(),
        dependencies: Vec::new(),
        risks: vec!["Stale evidence".into()],
        security_constraints: Vec::new(),
        test_strategy: "Test persisted receipts".into(),
        deployment_notes: String::new(),
        open_questions: Vec::new(),
        deferred_assumptions: Vec::new(),
        decisions: "User approved the exact plan hash".into(),
        spec_revision: "5".into(),
        approval_contract_sha256: "a".repeat(64),
        content_hash: "b".repeat(64),
        author_run_id: None,
        author_session_id: None,
    };
    let review = LifecyclePlanReviewView {
        feature_uid: "feature-uid".into(),
        snapshot,
        confirmation: None,
        review: None,
        verdict: None,
    };
    let context = egui::Context::default();
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 700.0),
            )),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                super::paint_plan_review(ui, &review);
            });
        },
    );
    output.textures_delta.clear();
    let rendered = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        rendered
            .iter()
            .any(|text| text == "Independent Plan Review")
    );
    assert!(rendered.iter().any(|text| text == "Confirmed plan"));
    assert!(rendered.iter().any(|text| text.contains("confirmed plan")));
    assert!(
        rendered
            .iter()
            .any(|text| text.contains("awaiting an independent reviewer"))
    );
    assert!(!rendered.iter().any(|text| text == "Confirm this plan"));
}
