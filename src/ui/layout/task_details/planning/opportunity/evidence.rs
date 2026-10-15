use egui::Ui;

pub(super) fn paint(ui: &mut Ui, proposal: &crate::core::opportunity::OpportunityV1) {
    ui.collapsing("Target and evidence", |ui| {
        ui.hyperlink_to("Open target repository", &proposal.target.canonical_remote);
        ui.label(format!("Source revision: {}", proposal.target.source_sha));
        if proposal.evidence_refs.is_empty() {
            ui.label("No evidence references were recorded.");
        }
        for evidence in &proposal.evidence_refs {
            ui.group(|ui| {
                ui.label(format!("Evidence {}", evidence.evidence_uid));
                ui.label(format!("SHA-256 {}", evidence.sha256));
                ui.label(match evidence.confidentiality {
                    crate::core::opportunity::EvidenceConfidentialityV1::Public => {
                        "Public evidence reference"
                    }
                    crate::core::opportunity::EvidenceConfidentialityV1::UserLocal => {
                        "User-local evidence reference; source content remains in this workspace"
                    }
                });
            });
        }
    });
}
