use super::theme;

pub(super) fn paint(ui: &mut egui::Ui, lifecycle: Option<&crate::ui::task_detail::LifecycleView>) {
    let Some(lifecycle) = lifecycle else {
        return;
    };
    let Some(evidence) = lifecycle.runtime_qa_evidence.as_ref() else {
        if let Some(assessment) = lifecycle.runtime_qa_applicability.as_ref() {
            ui.group(|ui| {
                ui.label(theme::section_heading("Runtime QA applicability"));
                ui.label("Not applicable · no visual QA pass was recorded");
                ui.label(format!("Reason · {}", assessment.reason));
                ui.label(format!(
                    "Independent reviewer · {} · Manager approval · {}",
                    assessment.reviewer_execution_id, assessment.approved_by
                ));
                ui.label(format!(
                    "Evidence · {}",
                    assessment.evidence_refs.join(", ")
                ));
            });
        }
        if let Some(error) = lifecycle.runtime_qa_evidence_error.as_deref() {
            ui.group(|ui| {
                ui.label(theme::section_heading("Runtime QA evidence unavailable"));
                ui.colored_label(theme::WARNING, error);
            });
        }
        return;
    };
    ui.group(|ui| {
        ui.label(theme::section_heading("Runtime QA evidence"));
        ui.label(format!(
            "Run · {} · {}",
            evidence.run_id,
            outcome(evidence.outcome)
        ));
        ui.label("Requirement coverage");
        for item in &evidence.coverage {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("{} · {:?}", item.requirement_id, item.status));
                if !item.evidence_refs.is_empty() {
                    ui.small(format!("Evidence: {}", item.evidence_refs.join(", ")));
                }
            });
        }
        ui.label(format!("Findings · {}", evidence.findings.len()));
        for finding in &evidence.findings {
            ui.label(format!(
                "{:?} · {:?} · {}",
                finding.severity, finding.category, finding.finding_uid
            ));
            ui.small(format!("Expected: {}", finding.expected));
            ui.small(format!("Observed: {}", finding.actual));
            ui.small(format!("Repair: {}", finding.smallest_fix));
            ui.small(format!(
                "Requirements: {}",
                finding.requirement_uids.join(", ")
            ));
        }
        ui.label(format!("Action trace · {} step(s)", evidence.actions.len()));
        for action in &evidence.actions {
            ui.small(format!(
                "#{} · {} · {} → {} · evidence {}",
                action.index,
                action.command,
                action.pre_state.as_deref().unwrap_or("unknown"),
                action.post_state.as_deref().unwrap_or("unknown"),
                action.evidence_ids.join(", ")
            ));
        }
        ui.label(format!("Screenshots · {}", evidence.screenshots.len()));
        for screenshot in &evidence.screenshots {
            ui.label(format!(
                "{} · {} × {}",
                screenshot.id, screenshot.width, screenshot.height
            ));
            ui.add(
                egui::Image::from_bytes(
                    format!("bytes://runtime-qa/{}/{}", evidence.run_id, screenshot.id),
                    screenshot.bytes.clone(),
                )
                .max_width(480.0),
            );
        }
    });
}

fn outcome(outcome: crate::runtime_qa::QaOutcome) -> &'static str {
    match outcome {
        crate::runtime_qa::QaOutcome::Pass => "Pass",
        crate::runtime_qa::QaOutcome::Fail => "Findings",
        crate::runtime_qa::QaOutcome::Incomplete => "Incomplete",
        crate::runtime_qa::QaOutcome::InfrastructureBlocked => "Blocked by infrastructure",
    }
}

#[cfg(test)]
mod tests {
    use super::paint;
    use crate::{
        core::lifecycle::{FeaturePhase, SpecReconciliationStatusV1, TaskPhase},
        project_runtime::AppSurface,
        runtime_qa::{
            QaApplicability, QaApplicabilityAssessmentV1, QaCoverageStatus, QaOutcome,
            QaRequirementCoverageV1,
        },
        ui::task_detail::{LifecycleView, RuntimeQaEvidenceView},
    };

    /// Production task-details boundary: project requirement evidence into visible QA state.
    /// Model assertions alone cannot detect a missing or mislabeled evidence panel.
    /// Synthetic in-memory data only; no Docker/browser/network dependencies; negligible setup.
    #[test]
    fn task_details_display_requirement_coverage_and_run_outcome() {
        let lifecycle = LifecycleView {
            feature_phase: FeaturePhase::Executing,
            task_phase: TaskPhase::AwaitingSpecReconciliation,
            parent_task_uid: None,
            parent_task: None,
            dependencies: Vec::new(),
            subtasks: Vec::new(),
            task_revision: 2,
            code_review: None,
            runtime_qa: None,
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
                coverage: vec![QaRequirementCoverageV1 {
                    requirement_id: "REQ-1".into(),
                    status: QaCoverageStatus::Observed,
                    evidence_refs: vec!["shot-1".into()],
                }],
                findings: Vec::new(),
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
        output.textures_delta.clear();
    }

    #[test]
    fn task_details_identify_headless_visual_qa_as_not_applicable_with_signoff() {
        let lifecycle = LifecycleView {
            feature_phase: FeaturePhase::Executing,
            task_phase: TaskPhase::AwaitingSpecReconciliation,
            parent_task_uid: None,
            parent_task: None,
            dependencies: Vec::new(),
            subtasks: Vec::new(),
            task_revision: 3,
            code_review: None,
            runtime_qa: None,
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
}
