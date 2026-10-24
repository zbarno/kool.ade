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
        ui.small(format!(
            "Target · {} · Platform · {:?} · Driver · {}",
            evidence
                .target_id
                .as_deref()
                .unwrap_or("unavailable (legacy report)"),
            evidence.platform,
            evidence.driver_version
        ));
        ui.small(format!("Source SHA · {}", evidence.code_sha));
        ui.small(format!(
            "Approved spec SHA · {}",
            evidence.approved_spec_hash
        ));
        ui.small(format!("Build SHA · {}", evidence.build_hash));
        ui.small(format!(
            "Display · {} × {} · {} dpi · {} · {}",
            evidence
                .display
                .width
                .map_or_else(|| "unknown".into(), |value| value.to_string()),
            evidence
                .display
                .height
                .map_or_else(|| "unknown".into(), |value| value.to_string()),
            evidence
                .display
                .dpi
                .map_or_else(|| "unknown".into(), |value| value.to_string()),
            evidence.display.theme,
            evidence.display.locale
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
        ui.label(format!(
            "Independent visual review · {} screenshot(s) inspected",
            evidence.reviewed_screenshot_ids.len()
        ));
        for item in &evidence.review_coverage {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} · reviewer {:?}",
                    item.requirement_id, item.status
                ));
                ui.small(format!("Evidence: {}", item.evidence_refs.join(", ")));
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
        ui.label(format!("Assertions · {}", evidence.assertions.len()));
        for assertion in &evidence.assertions {
            ui.small(format!(
                "{} · {:?} · requirements {}",
                assertion.id,
                assertion.status,
                assertion.requirement_ids.join(", ")
            ));
            ui.small(format!("Expected: {}", assertion.expected));
            ui.small(format!("Observed: {}", assertion.observed));
            ui.small(format!("Evidence: {}", assertion.evidence_refs.join(", ")));
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
#[path = "lifecycle_qa/tests.rs"]
mod tests;
