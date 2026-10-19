use super::Action;

pub(super) fn paint(ui: &mut egui::Ui, action: &Action, title: &str) {
    ui.heading(format!("{title}: {}", action.id));
    let Some(snapshot) = &action.plan_snapshot else {
        ui.label(
            "The plan snapshot could not be summarized. Review the complete specification below.",
        );
        return;
    };
    ui.label("Review the exact plan snapshot before confirming it. Task generation waits for an independent review.");
    field(ui, "Original user goal", &snapshot.original_goal);
    field(ui, "Intent", &snapshot.intent);
    field(ui, "Expected outcome", &snapshot.outcome);
    field(ui, "Current behavior", &snapshot.current_state);
    field(ui, "Target behavior", &snapshot.target_behavior);
    list(ui, "In scope", &snapshot.scope);
    list(ui, "Out of scope", &snapshot.non_goals);
    ui.collapsing("Requirements", |ui| {
        for requirement in &snapshot.requirements {
            ui.label(format!("{}: {}", requirement.stable_id, requirement.text));
            field(ui, "Rationale", &requirement.rationale);
            if !requirement.acceptance_criteria_ids.is_empty() {
                field(
                    ui,
                    "Acceptance criteria links",
                    &requirement.acceptance_criteria_ids.join(", "),
                );
            }
        }
    });
    list(ui, "How success is measured", &snapshot.acceptance_criteria);
    field(ui, "Architecture", &snapshot.architecture);
    list(ui, "Dependencies", &snapshot.dependencies);
    list(ui, "Risks", &snapshot.risks);
    list(ui, "Security constraints", &snapshot.security_constraints);
    field(ui, "Test strategy", &snapshot.test_strategy);
    field(ui, "Deployment notes", &snapshot.deployment_notes);
    list(ui, "Open questions", &snapshot.open_questions);
    list(ui, "Deferred assumptions", &snapshot.deferred_assumptions);
    field(ui, "Recorded decisions", &snapshot.decisions);
}

fn field(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.collapsing(label, |ui| {
        ui.label(value);
    });
}

fn list(ui: &mut egui::Ui, label: &str, values: &[String]) {
    ui.collapsing(format!("{label} ({})", values.len()), |ui| {
        if values.is_empty() {
            ui.label("None recorded.");
        } else {
            for value in values {
                ui.label(format!("• {value}"));
            }
        }
    });
}

pub(super) fn paint_review(
    ui: &mut egui::Ui,
    verdict: &crate::harness::prompt_catalog::ReviewVerdictV1,
) {
    ui.heading("Independent review findings");
    if verdict.outcome == Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable) {
        ui.label("The reviewer could not complete a defensible assessment. Task generation remains blocked.");
    }
    for finding in &verdict.findings {
        ui.collapsing(
            format!(
                "{} · {:?} · {}",
                finding.id, finding.severity, finding.requirement
            ),
            |ui| {
                field(ui, "Location", &finding.location);
                field(ui, "Finding", &finding.proposed_correction);
                if let Some(repro) = &finding.repro {
                    field(ui, "Counterexample", repro);
                }
                list(ui, "Evidence references", &finding.evidence);
            },
        );
    }
    list(ui, "Untested requirements", &verdict.untested);
}
