use crate::ui::{Surface, task_detail};

pub(super) fn paint(
    ui: &mut egui::Ui,
    surface: &mut dyn Surface,
    ticket: &str,
    view: &task_detail::ViewModel,
) {
    let Some(review) = view.lifecycle_spec_proposal.as_ref() else {
        return;
    };
    ui.group(|ui| {
        ui.label(egui::RichText::new("Specification changes need your review").strong());
        ui.label(&review.reason);
        if let Some(proposal) = &review.proposal {
            for delta in &proposal.deltas {
                ui.collapsing(&delta.requirement_id, |ui| {
                    ui.label(egui::RichText::new("Current").strong());
                    ui.label(&delta.old);
                    ui.label(egui::RichText::new("Proposed").strong());
                    ui.label(&delta.new);
                    ui.label(format!("Classification: {:?}; approval effect: {:?}", delta.classification, delta.approval_effect));
                    for evidence in &delta.evidence {
                        ui.label(format!("Evidence: {evidence}"));
                    }
                });
            }
            if ui.button("Approve and apply specification changes").clicked() {
                surface.dispatch(crate::ui::ApplicationCommand::TaskDetail(
                    task_detail::Command::ApproveSpecReconciliation {
                        ticket: ticket.to_owned(),
                        expected_revision: review.expected_revision,
                        proposal_sha256: review.proposal_sha256.clone(),
                        base_spec_sha256: review.base_spec_sha256.clone(),
                    },
                ));
            }
        } else {
            ui.label("The saved proposal is unavailable or failed integrity validation. Re-run reconciliation before approval.");
        }
    });
}
