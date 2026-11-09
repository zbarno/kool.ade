//! Plain-English consent panel for the "Prepare this computer" flow.
//!
//! Appears on a step's detail view exactly when the flow is holding for
//! THAT step's explicit approval. Copy comes only from the curated static
//! catalog or scanned task text ([`crate::app::setup_explain`]); no model
//! ever writes consent copy. Acceptance re-enters the existing scoped
//! approval flow; decline tells the flow to stop proposing this operation.

use crate::ui::{ApplicationCommand, Surface};

pub(super) fn paint_if_waiting(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    task: &crate::persistence::setup_tasks::LocalTask,
) {
    let Some(flow) = surface.setup_flow_position() else {
        return;
    };
    let waiting_id = match &flow.phase {
        crate::persistence::setup_tasks::FlowPhaseV1::WaitingForConsent { task_id } => {
            task_id.clone()
        }
        _ => return,
    };
    if waiting_id != task.id {
        return;
    }
    let explanation = crate::app::setup_explain::SetupExplanation::explain(task);
    ui.group(|ui| {
        ui.label(
            egui::RichText::new("Kool.ad/e would like to make a change").strong(),
        );
        ui.add_space(4.0);
        ui.label("What changes:");
        for item in explanation.what_changes() {
            ui.label(format!("  • {}", item));
        }
        ui.label(format!("Why: {}", explanation.why()));
        ui.label(format!("Security: {}", explanation.security()));
        ui.collapsing("More details — reversibility and decline consequences", |ui| {
            reveal_more_details(ui, &explanation);
        });
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(
                    egui::Button::new("Allow this change").fill(crate::ui::theme::ACTION),
                )
                .clicked()
            {
                surface.dispatch(ApplicationCommand::ApproveSetupTaskFix {
                    task_id: task.id.clone(),
                });
            }
            if ui
                .add(
                    egui::Button::new("Decline").fill(crate::ui::theme::WARNING.gamma_multiply(0.25)),
                )
                .clicked()
            {
                surface.dispatch(ApplicationCommand::DeclineSetupOperation {
                    task_id: task.id.clone(),
                });
            }
        });
        ui.label(crate::ui::theme::helper_text(
            "Declining is safe: nothing changes on this machine, and the flow moves past this step until you say otherwise.",
        ));
    });
}
fn reveal_more_details(
    ui: &mut egui::Ui,
    explanation: &crate::app::setup_explain::SetupExplanation,
) {
    ui.indent("setup-consent-details", |ui| {
        ui.label(format!("Reversible: {}", explanation.reversibility()));
        if let Some(note) = explanation.rollback_note() {
            ui.label(format!("Rollback: {}", note));
        }
        ui.label(format!("If you decline: {}", explanation.if_declined()));
    });
}
