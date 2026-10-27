//! In-memory, one-use machine repair consent and redacted audit receipts.
//!
//! The controller renders the approval control itself and derives machine and
//! daemon bindings internally. No host-shell executor or agent-facing approval
//! tool is defined here.

mod audit;
mod binding;
mod capability;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

use std::{collections::HashMap, time::SystemTime};

use serde::Serialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::app::bootstrap_repair_policy::{self, Proposal};

pub(crate) use self::capability::{
    ApprovalPreview, ApprovalRequest, ApprovedAction, ImagePreparationReceipt,
    engine_identity_fingerprint,
};
use self::capability::{CONSENT_TTL_MS, Pending};

/// The application controller owns this gate; tickets exist only in memory.
pub(crate) struct ApprovalGate {
    approvals: HashMap<Uuid, Pending>,
    audit_path: Result<std::path::PathBuf, String>,
}

impl ApprovalGate {
    pub(crate) fn new() -> Self {
        Self {
            approvals: HashMap::new(),
            audit_path: crate::persistence::setup_tasks::machine_setup_state_dir()
                .map(|directory| repair_audit_path(&directory))
                .map_err(|error| error.to_string()),
        }
    }

    #[cfg(test)]
    fn at_path(audit_path: std::path::PathBuf) -> Self {
        Self {
            approvals: HashMap::new(),
            audit_path: Ok(audit_path),
        }
    }

    fn audit_path_ref(&self) -> anyhow::Result<&std::path::Path> {
        self.audit_path.as_deref().map_err(|error| {
            anyhow::anyhow!("Secure machine repair audit storage is unavailable: {error}")
        })
    }

    /// Probe asynchronously before showing the confirmation UI. The preview
    /// binds the exact locked image, local daemon and machine identity.
    #[allow(dead_code)] // Consumed by the dependent image setup UI in #345.
    pub(crate) async fn preview(&self, proposal: &Proposal) -> anyhow::Result<ApprovalPreview> {
        let plan = bootstrap_repair_policy::validate(proposal, binding::current_host())?;
        anyhow::ensure!(
            plan.requires_consent,
            "Read-only operations do not require repair approval"
        );
        let binding = binding::MachineBinding::current(proposal).await?;
        binding.validate(now_unix_ms()?)?;
        Ok(ApprovalPreview {
            proposal: proposal.clone(),
            binding,
        })
    }

    /// Persist the user request and create a one-use ticket off the UI frame.
    #[allow(dead_code)] // Consumed by the dependent image setup UI in #345.
    pub(crate) fn grant(&mut self, request: ApprovalRequest) -> anyhow::Result<Uuid> {
        let now_ms = now_unix_ms()?;
        request.binding.validate(now_ms)?;
        self.approve_with_binding(&request.proposal, &request.binding, now_ms)
    }

    /// Consume a ticket before execution. The machine, principal, endpoint and
    /// daemon are re-derived now, so stale or replayed approvals fail closed.
    #[allow(dead_code)] // Consumed by the dependent image setup UI in #345.
    pub(crate) async fn consume(
        &mut self,
        id: Uuid,
        proposal: &Proposal,
    ) -> anyhow::Result<ApprovedAction> {
        let pending = self
            .approvals
            .remove(&id)
            .ok_or_else(|| anyhow::anyhow!("Machine repair approval is missing or already used"))?;
        let binding = match binding::MachineBinding::current(proposal).await {
            Ok(binding) => binding,
            Err(error) => {
                audit::append(
                    self.audit_path_ref()?,
                    pending.operation,
                    &pending.prerequisite_id,
                    &pending.proposal_digest,
                    &pending.binding_digest,
                    "rejected",
                    now_unix_ms()?,
                )?;
                return Err(error);
            }
        };
        self.consume_pending(pending, proposal, &binding, now_unix_ms()?)
    }

    #[allow(dead_code)] // Called when the dependent #345 operation is cancelled.
    pub(crate) fn cancel(&mut self, id: Uuid) -> anyhow::Result<()> {
        if let Some(pending) = self.approvals.remove(&id) {
            audit::append(
                self.audit_path_ref()?,
                pending.operation,
                &pending.prerequisite_id,
                &pending.proposal_digest,
                &pending.binding_digest,
                "cancelled",
                now_unix_ms()?,
            )?;
        }
        Ok(())
    }

    fn approve_with_binding(
        &mut self,
        proposal: &Proposal,
        binding: &binding::MachineBinding,
        now_ms: u64,
    ) -> anyhow::Result<Uuid> {
        let plan = bootstrap_repair_policy::validate(proposal, binding.host)?;
        anyhow::ensure!(
            plan.requires_consent,
            "Read-only operations do not require repair approval"
        );
        binding.validate(now_ms)?;
        anyhow::ensure!(
            proposal.host == binding.host,
            "Host identity changed during approval"
        );
        let proposal_digest = fingerprint(proposal)?;
        let binding_digest = binding.consent_digest()?;
        audit::append(
            self.audit_path_ref()?,
            plan.operation,
            &proposal.prerequisite_id,
            &proposal_digest,
            &binding_digest,
            "approved",
            now_ms,
        )?;
        let id = Uuid::new_v4();
        self.approvals.insert(
            id,
            Pending {
                proposal_digest,
                binding_digest,
                expires_at_unix_ms: now_ms.saturating_add(CONSENT_TTL_MS),
                operation: plan.operation,
                prerequisite_id: proposal.prerequisite_id.clone(),
            },
        );
        Ok(id)
    }

    fn consume_pending(
        &mut self,
        pending: Pending,
        proposal: &Proposal,
        current: &binding::MachineBinding,
        now_ms: u64,
    ) -> anyhow::Result<ApprovedAction> {
        let valid = current.validate(now_ms).is_ok()
            && now_ms <= pending.expires_at_unix_ms
            && proposal.host == current.host
            && fingerprint(proposal).as_deref().ok() == Some(pending.proposal_digest.as_str())
            && current.consent_digest().as_deref().ok() == Some(pending.binding_digest.as_str())
            && bootstrap_repair_policy::validate(proposal, current.host)
                .is_ok_and(|plan| plan.requires_consent && plan.operation == pending.operation);
        audit::append(
            self.audit_path_ref()?,
            pending.operation,
            &pending.prerequisite_id,
            &pending.proposal_digest,
            &pending.binding_digest,
            if valid { "consumed" } else { "rejected" },
            now_ms,
        )?;
        anyhow::ensure!(
            valid,
            "Machine repair consent expired or no longer matches the verified configuration"
        );
        Ok(ApprovedAction {
            operation: pending.operation,
            prerequisite_id: proposal.prerequisite_id.clone(),
            image_profile: proposal.image_profile.clone(),
            image_reference: current.image_reference.clone(),
            image_digest: current.image_digest.clone(),
            image_platform: current.image_platform.clone(),
            capacity_probe_required: current.capacity_probe_required,
            engine_fingerprint: current.probe_hash.clone(),
            engine_identity_fingerprint: current.engine_identity_fingerprint.clone(),
            binding_digest: pending.binding_digest.clone(),
            proposal_digest: pending.proposal_digest.clone(),
        })
    }

    #[cfg(test)]
    fn approve_for_test(
        &mut self,
        proposal: &Proposal,
        binding: &binding::MachineBinding,
        now_ms: u64,
    ) -> anyhow::Result<Uuid> {
        self.approve_with_binding(proposal, binding, now_ms)
    }

    #[cfg(test)]
    fn consume_for_test(
        &mut self,
        id: Uuid,
        proposal: &Proposal,
        current: &binding::MachineBinding,
        now_ms: u64,
    ) -> anyhow::Result<ApprovedAction> {
        let pending = self
            .approvals
            .remove(&id)
            .ok_or_else(|| anyhow::anyhow!("Machine repair approval is missing or already used"))?;
        self.consume_pending(pending, proposal, current, now_ms)
    }
}

impl Default for ApprovalGate {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn repair_audit_path(state_directory: &std::path::Path) -> std::path::PathBuf {
    state_directory.join("repair-approval-audit.json")
}

#[cfg(test)]
pub(crate) fn append_audit_for_test(path: &std::path::Path) -> anyhow::Result<()> {
    audit::append(
        path,
        bootstrap_repair_policy::Operation::PrepareLockedImage,
        &format!("setup:images:general:amd64:{}", "e".repeat(64)),
        &"a".repeat(64),
        &"b".repeat(64),
        "approved",
        1,
    )
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_machine_id() -> anyhow::Result<String> {
    binding::linux_machine_id()
}

fn fingerprint<T: Serialize>(value: &T) -> anyhow::Result<String> {
    let bytes = serde_json::to_vec(value)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn now_unix_ms() -> anyhow::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis()
        .try_into()?)
}
