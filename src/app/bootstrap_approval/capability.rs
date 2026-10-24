use crate::app::bootstrap_repair_policy::{Operation, Proposal};

use super::{binding, now_unix_ms};

pub(super) const CONSENT_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Debug)]
pub(super) struct Pending {
    pub(super) proposal_digest: String,
    pub(super) binding_digest: String,
    pub(super) expires_at_unix_ms: u64,
    pub(super) operation: Operation,
    pub(super) prerequisite_id: String,
}

/// Opaque, non-cloneable authority for one reviewed operation.
#[allow(dead_code)] // Consumed by the dependent image setup UI in #345.
#[derive(Debug)]
pub(crate) struct ApprovedAction {
    pub(super) operation: Operation,
    pub(super) prerequisite_id: String,
    pub(super) image_profile: Option<String>,
    pub(super) image_reference: String,
    pub(super) image_digest: String,
    pub(super) image_platform: String,
    pub(super) engine_fingerprint: String,
    pub(super) binding_digest: String,
    pub(super) proposal_digest: String,
}

#[allow(dead_code)] // Consumed by the dependent image setup UI in #345.
impl ApprovedAction {
    pub(crate) fn operation(&self) -> Operation {
        self.operation
    }

    pub(crate) fn prerequisite_id(&self) -> &str {
        &self.prerequisite_id
    }

    pub(crate) fn image_profile(&self) -> Option<&str> {
        self.image_profile.as_deref()
    }

    pub(crate) fn image_reference(&self) -> &str {
        &self.image_reference
    }

    pub(crate) fn image_digest(&self) -> &str {
        &self.image_digest
    }

    pub(crate) fn image_platform(&self) -> &str {
        &self.image_platform
    }

    pub(crate) fn engine_fingerprint(&self) -> &str {
        &self.engine_fingerprint
    }

    pub(crate) fn binding_digest(&self) -> &str {
        &self.binding_digest
    }

    /// Re-probe immediately before the side effect and compare the endpoint,
    /// proposal, daemon and digest locked into this opaque capability.
    pub(crate) fn prepare_image(self, proposal: &Proposal) -> anyhow::Result<()> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let verified = binding::VerifiedEngine::current(proposal).await?;
            self.validate_binding(proposal, verified.binding(), now_unix_ms()?)?;
            verified.run_smoke_test(&self).await
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        profile: &str,
        prerequisite_id: &str,
        image: &crate::execution::docker::images::SelectedImage,
        engine_fingerprint: &str,
    ) -> Self {
        Self {
            operation: Operation::PrepareLockedImage,
            prerequisite_id: prerequisite_id.to_owned(),
            image_profile: Some(profile.to_owned()),
            image_reference: image.reference.clone(),
            image_digest: image.digest.clone(),
            image_platform: image.platform.clone(),
            engine_fingerprint: engine_fingerprint.to_owned(),
            binding_digest: "b".repeat(64),
            proposal_digest: "c".repeat(64),
        }
    }

    pub(super) fn validate_binding(
        &self,
        proposal: &Proposal,
        current: &binding::MachineBinding,
        now_ms: u64,
    ) -> anyhow::Result<()> {
        current.validate(now_ms)?;
        anyhow::ensure!(
            super::fingerprint(proposal)? == self.proposal_digest
                && current.consent_digest()? == self.binding_digest
                && current.image_reference == self.image_reference
                && current.image_digest == self.image_digest
                && current.image_platform == self.image_platform,
            "Approved operation, endpoint or image digest changed before preparation"
        );
        Ok(())
    }
}

/// A controller-created snapshot shown to the user before they grant consent.
/// Its fields cannot be substituted by the UI caller.
#[allow(dead_code)] // Consumed by the dependent image setup UI in #345.
pub(crate) struct ApprovalPreview {
    pub(super) proposal: Proposal,
    pub(super) binding: binding::MachineBinding,
}

impl ApprovalPreview {
    #[allow(dead_code)] // Consumed by the dependent image setup UI in #345.
    pub(crate) fn show_confirmation(&self, ui: &mut egui::Ui) -> Option<ApprovalRequest> {
        ui.heading("Review approved image preparation");
        ui.label(format!(
            "This machine's verified local rootless Docker daemon ({}) will download the pinned image below.",
            self.binding.daemon_id
        ));
        ui.label(format!(
            "Machine fingerprint: {}",
            &self.binding.machine_hash[..12]
        ));
        ui.label(format!(
            "Image: {}@{} ({})",
            self.binding.image_reference, self.binding.image_digest, self.binding.image_platform
        ));
        ui.label(
            "The image will run a bounded smoke check as a non-root user with network access disabled. No host command or elevation is involved.",
        );
        ui.button("Approve image preparation")
            .clicked()
            .then(|| ApprovalRequest {
                proposal: self.proposal.clone(),
                binding: self.binding.clone(),
            })
    }
}

/// Created only by the controller-owned confirmation widget. It is sent to
/// the background controller for audit and ticket creation after the UI frame.
#[allow(dead_code)] // Consumed by the dependent image setup UI in #345.
pub(crate) struct ApprovalRequest {
    pub(super) proposal: Proposal,
    pub(super) binding: binding::MachineBinding,
}

impl ApprovalRequest {
    pub(crate) fn proposal(&self) -> &Proposal {
        &self.proposal
    }
}
