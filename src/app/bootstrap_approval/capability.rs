use crate::app::bootstrap_repair_policy::{Host, Operation, Proposal};
use crate::execution::docker::images::RuntimeProfile;
use sha2::{Digest, Sha256};

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

/// Positive evidence returned only after a locked image passed its isolated
/// smoke test and final daemon identity checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImagePreparationReceipt {
    pub(crate) profile: RuntimeProfile,
    pub(crate) architecture: String,
    pub(crate) image_digest: String,
    pub(crate) engine_identity_fingerprint: String,
}

/// Match the stable daemon identity used by Docker readiness evidence. This
/// deliberately excludes endpoint-probe details and the selected image.
pub(crate) fn engine_identity_fingerprint(
    endpoint_fingerprint: &str,
    daemon_id: &str,
) -> Option<String> {
    if !is_lower_sha256(endpoint_fingerprint) || daemon_id.trim().is_empty() {
        return None;
    }
    let daemon_identity_fingerprint = format!("{:x}", Sha256::digest(daemon_id.as_bytes()));
    let identity = serde_json::to_vec(&(
        "koolade-engine-identity-v1",
        endpoint_fingerprint,
        daemon_identity_fingerprint,
    ))
    .ok()?;
    Some(format!("{:x}", Sha256::digest(identity)))
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
    pub(super) capacity_probe_required: bool,
    pub(super) engine_fingerprint: String,
    pub(super) engine_identity_fingerprint: String,
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

    pub(crate) fn capacity_probe_required(&self) -> bool {
        self.capacity_probe_required
    }

    pub(crate) fn engine_fingerprint(&self) -> &str {
        &self.engine_fingerprint
    }

    pub(crate) fn engine_identity_fingerprint(&self) -> &str {
        &self.engine_identity_fingerprint
    }

    pub(crate) fn binding_digest(&self) -> &str {
        &self.binding_digest
    }

    /// Re-probe immediately before the side effect and compare the endpoint,
    /// proposal, daemon and digest locked into this opaque capability.
    pub(crate) fn prepare_image(
        self,
        proposal: &Proposal,
        pull_control: &crate::execution::docker::engine::ImagePullControl,
    ) -> anyhow::Result<ImagePreparationReceipt> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(async {
            let verified = binding::VerifiedEngine::current(proposal).await?;
            self.validate_binding(proposal, verified.binding(), now_unix_ms()?)?;
            verified.run_smoke_test(self, pull_control).await
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        profile: &str,
        prerequisite_id: &str,
        image: &crate::execution::docker::images::SelectedImage,
        engine_fingerprint: &str,
        engine_identity_fingerprint: &str,
    ) -> Self {
        Self {
            operation: Operation::PrepareLockedImage,
            prerequisite_id: prerequisite_id.to_owned(),
            image_profile: Some(profile.to_owned()),
            image_reference: image.reference.clone(),
            image_digest: image.digest.clone(),
            image_platform: image.platform.clone(),
            capacity_probe_required: false,
            engine_fingerprint: engine_fingerprint.to_owned(),
            engine_identity_fingerprint: engine_identity_fingerprint.to_owned(),
            binding_digest: "b".repeat(64),
            proposal_digest: "c".repeat(64),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_capacity_probe(
        profile: &str,
        prerequisite_id: &str,
        image: &crate::execution::docker::images::SelectedImage,
        engine_fingerprint: &str,
        engine_identity_fingerprint: &str,
        capacity_probe_required: bool,
    ) -> Self {
        let mut action = Self::for_test(
            profile,
            prerequisite_id,
            image,
            engine_fingerprint,
            engine_identity_fingerprint,
        );
        action.capacity_probe_required = capacity_probe_required;
        action
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
                && current.engine_identity_fingerprint == self.engine_identity_fingerprint
                && current.image_reference == self.image_reference
                && current.image_digest == self.image_digest
                && current.image_platform == self.image_platform
                && current.capacity_probe_required == self.capacity_probe_required,
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
        let engine_description = match self.binding.host {
            Host::Linux => "verified local rootless Docker Engine",
            Host::MacOs | Host::Windows => "verified local Docker Desktop Linux VM",
        };
        ui.label(format!(
            "This machine's {engine_description} ({}) will prepare the pinned image below. It will pull the image only if its exact digest is absent.",
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
        let registry = self
            .binding
            .image_reference
            .split('@')
            .next()
            .unwrap_or(&self.binding.image_reference);
        ui.label(format!("Registry source: {registry}"));
        ui.label(format!(
            "Approximate cold-cache transfer: {}. Docker may reuse existing layers.",
            human_bytes(self.binding.estimated_pull_bytes)
        ));
        if self.binding.capacity_probe_required {
            ui.label(format!(
                "Docker Desktop cannot measure VM free space before this first General image. Its bounded lock estimate is {} transfer and {} added storage.",
                human_bytes(self.binding.estimated_pull_bytes),
                human_bytes(self.binding.estimated_storage_bytes)
            ));
            ui.label(
                "The app will measure VM capacity after pulling the pinned General image and before smoke verification. If the pull runs out of space or capacity cannot be measured, preparation fails and no successful receipt is recorded. Other images remain blocked until capacity is measurable.",
            );
        } else {
            let available = self
                .binding
                .disk_available_bytes
                .map(human_bytes)
                .unwrap_or_else(|| "unknown".into());
            ui.label(format!(
                "Approximate added storage: {}; keep at least {} free for the runtime after preparation. Current Docker storage free space: {}.",
                human_bytes(self.binding.estimated_storage_bytes),
                human_bytes(self.binding.minimum_disk_bytes),
                available
            ));
            ui.label(format!(
                "Approval requires about {} free before download; storage is rechecked before the pull.",
                human_bytes(
                    self.binding
                        .minimum_disk_bytes
                        .saturating_add(self.binding.estimated_storage_bytes)
                )
            ));
        }
        ui.label(
            "Transfer estimates sum the exact digest's compressed layers and config. Storage is an approximate 2× layer-size reserve; actual unpacked use varies.",
        );
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

fn human_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    if bytes as f64 >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB)
    } else {
        format!("{:.0} MiB", bytes as f64 / MIB)
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
