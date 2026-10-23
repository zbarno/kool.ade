use super::paint;
use crate::{
    core::lifecycle::{FeaturePhase, SpecReconciliationStatusV1, TaskPhase},
    project_runtime::AppSurface,
    runtime_qa::{
        QaApplicability, QaApplicabilityAssessmentV1, QaAssertionStatus, QaAssertionV1,
        QaCoverageStatus, QaDisplayV1, QaOutcome, QaPlatform, QaRequirementCoverageV1,
    },
    ui::task_detail::{LifecycleView, RuntimeQaEvidenceView},
};

/// Production task-details boundary: project requirement evidence into visible QA state.
/// Model assertions alone cannot detect a missing or mislabeled evidence panel.
/// Synthetic in-memory data only; no Docker/browser/network dependencies; negligible setup.
#[test]
fn task_details_display_requirement_coverage_and_run_outcome() {
    let lifecycle = LifecycleView {
        feature_uid: "feature-uid".into(),
        feature_phase: FeaturePhase::Executing,
        task_phase: TaskPhase::AwaitingSpecReconciliation,
        parent_task_uid: None,
        parent_task: None,
        dependencies: Vec::new(),
        subtasks: Vec::new(),
        task_revision: 2,
        code_review: None,
        code_review_attempts: Vec::new(),
        current_code_review: None,
        code_review_dispositions: Vec::new(),
        code_review_load_error: None,
        runtime_qa: None,
        runtime_qa_run_id: None,
        spec_reconciliation: None,
        current_spec_hash: None,
        implementation_head_sha: None,
        approved_target_hash: None,
        deployed_spec_hash: None,
        deployment_status: None,
        deployment_observed_at_ms: None,
        spec_revision: None,
        spec_status: Some(SpecReconciliationStatusV1::Synchronized),
        pending_spec_triggers: 0,
        specification_deltas: Vec::new(),
        pull_request_status: None,
        merged_commit_sha: None,
        runtime_qa_evidence: Some(RuntimeQaEvidenceView {
            run_id: "run-id".into(),
            outcome: QaOutcome::Pass,
            target_id: Some("web-preview".into()),
            code_sha: "a".repeat(40),
            approved_spec_hash: "b".repeat(64),
            build_hash: "c".repeat(64),
            platform: QaPlatform::Web,
            driver_version: "browser-driver/v1".into(),
            display: QaDisplayV1 {
                width: Some(1280),
                height: Some(720),
                dpi: Some(96),
                theme: "dark".into(),
                locale: "en-US".into(),
            },
            coverage: vec![QaRequirementCoverageV1 {
                requirement_id: "REQ-1".into(),
                status: QaCoverageStatus::Observed,
                evidence_refs: vec!["shot-1".into()],
            }],
            review_coverage: vec![crate::runtime_qa::QaReviewRequirementCoverageV1 {
                requirement_id: "REQ-1".into(),
                status: crate::runtime_qa::QaReviewCoverageStatusV1::Pass,
                evidence_refs: vec!["shot-1".into(), "0".into()],
            }],
            reviewed_screenshot_ids: vec!["shot-1".into()],
            findings: Vec::new(),
            assertions: vec![QaAssertionV1 {
                id: "assertion-1".into(),
                scenario_ids: vec!["scenario-1".into()],
                requirement_ids: vec!["REQ-1".into()],
                expected: "The dialog closes".into(),
                observed: "The dialog closed after Escape".into(),
                status: QaAssertionStatus::Passed,
                evidence_refs: vec!["action-0".into()],
            }],
            actions: Vec::new(),
            screenshots: Vec::new(),
        }),
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
                paint(ui, Some(&lifecycle));
            });
        },
    );
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
            .any(|text| text.contains("Run · run-id · Pass"))
    );
    assert!(
        rendered
            .iter()
            .any(|text| text.contains("REQ-1 · Observed"))
    );
    assert!(
        rendered
            .iter()
            .any(|text| text.contains("1 screenshot(s) inspected"))
    );
    assert!(
        rendered
            .iter()
            .any(|text| text.contains("REQ-1 · reviewer Pass"))
    );
    assert!(
        rendered
            .iter()
            .any(|text| text.contains("Target · web-preview"))
    );
    assert!(rendered.iter().any(|text| text.contains("Platform · Web")));
    assert!(
        rendered
            .iter()
            .any(|text| text.contains("The dialog closes"))
    );
    assert!(
        rendered
            .iter()
            .any(|text| text.contains("The dialog closed after Escape"))
    );
    output.textures_delta.clear();
}

#[test]
fn task_details_identify_headless_visual_qa_as_not_applicable_with_signoff() {
    let lifecycle = LifecycleView {
        feature_uid: "feature-uid".into(),
        feature_phase: FeaturePhase::Executing,
        task_phase: TaskPhase::AwaitingSpecReconciliation,
        parent_task_uid: None,
        parent_task: None,
        dependencies: Vec::new(),
        subtasks: Vec::new(),
        task_revision: 3,
        code_review: None,
        code_review_attempts: Vec::new(),
        current_code_review: None,
        code_review_dispositions: Vec::new(),
        code_review_load_error: None,
        runtime_qa: None,
        runtime_qa_run_id: None,
        spec_reconciliation: None,
        current_spec_hash: None,
        implementation_head_sha: None,
        approved_target_hash: None,
        deployed_spec_hash: None,
        deployment_status: None,
        deployment_observed_at_ms: None,
        spec_revision: None,
        spec_status: None,
        pending_spec_triggers: 0,
        specification_deltas: Vec::new(),
        pull_request_status: None,
        merged_commit_sha: None,
        runtime_qa_evidence: None,
        runtime_qa_evidence_error: None,
        runtime_qa_applicability: Some(QaApplicabilityAssessmentV1 {
            schema_version: 1,
            task_uid: "00000000-0000-4000-8000-000000000001".into(),
            target_id: "service-api".into(),
            app_surface: AppSurface::HeadlessService,
            code_sha: "a".repeat(40),
            approved_spec_hash: "b".repeat(64),
            applicability: QaApplicability::NotApplicable,
            reason: "Headless API has no visual surface".into(),
            evidence_refs: vec!["target:service-api".into()],
            reviewer_execution_id: "00000000-0000-4000-8000-000000000002".into(),
            reviewer_session_id: "00000000-0000-4000-8000-000000000003".into(),
            approved_by: "00000000-0000-4000-8000-000000000004".into(),
            manager_session_id: "00000000-0000-4000-8000-000000000005".into(),
            recorded_at_unix_ms: 1,
        }),
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
                paint(ui, Some(&lifecycle));
            });
        },
    );
    let rendered = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(rendered.iter().any(|text| text.contains("Not applicable")));
    assert!(rendered.iter().any(|text| text.contains("Headless API")));
    assert!(
        rendered
            .iter()
            .any(|text| text.contains("Manager approval"))
    );
    output.textures_delta.clear();
}
