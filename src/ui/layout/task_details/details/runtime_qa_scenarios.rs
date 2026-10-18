use crate::ui::{Surface, task_detail};

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    ticket: &str,
    view: &task_detail::ViewModel,
) {
    let Some(proposal) = view.runtime_qa_scenario_proposal.as_ref() else {
        return;
    };
    ui.group(|ui| {
        ui.label(crate::ui::theme::section_heading("Runtime QA scenarios"));
        if proposal.scenarios.is_empty() {
            ui.label("Scenarios are synthesized from the approved criteria and finished build evidence. Review and approve before any QA run.");
            if ui.button("Generate scenario proposal").clicked() {
                surface.dispatch(crate::ui::ApplicationCommand::GenerateRuntimeQaScenarios { ticket: ticket.to_owned() });
            }
            return;
        }
        ui.label(format!("Proposal {} · task revision {}", proposal.proposal_sha256.as_deref().unwrap_or("integrity unavailable"), proposal.task_revision));
        if proposal.stale {
            ui.colored_label(crate::ui::theme::WARNING, "This proposal belongs to an earlier task revision and cannot be approved. Create a fresh Runtime QA request before continuing.");
            return;
        }
        for (index, scenario) in proposal.scenarios.iter().enumerate() {
            ui.collapsing(format!("Scenario {} · {}", index + 1, scenario.id), |ui| {
                ui.label(format!("Requirements: {}", scenario.requirement_ids.join(", ")));
                for (step_index, step) in scenario.steps.iter().enumerate() {
                    ui.label(format!("{}. {:?} → {}", step_index + 1, step.driver_action, step.expected_state.as_deref().unwrap_or("no state assertion")));
                }
                if scenario.redaction_regions.is_empty() {
                    ui.colored_label(crate::ui::theme::WARNING, "Screenshots from this scenario are unredacted.");
                } else {
                    for (mask_index, region) in scenario.redaction_regions.iter().enumerate() {
                        ui.label(format!("Screenshot redaction {}: x={}, y={}, width={}, height={}", mask_index + 1, region.x, region.y, region.width, region.height));
                    }
                }
                for result in &scenario.expected_results { ui.label(format!("Expected: {result}")); }
            });
        }
        if proposal.approved {
            ui.label("Approved exact proposal. Runtime QA may start after the dispatcher revalidates it.");
        } else if ui.button("Approve these exact scenarios").clicked() {
            let Some(proposal_sha256) = proposal.proposal_sha256.clone() else { return; };
            surface.dispatch(crate::ui::ApplicationCommand::ApproveRuntimeQaScenarios {
                ticket: ticket.to_owned(),
                expected_task_revision: proposal.task_revision,
                proposal_sha256,
            });
        }
    });
}
