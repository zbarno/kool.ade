use egui::Ui;

use super::super::{attempt::AttemptContext, identity::HostIdentity, plan::RemediationPlan};

pub(crate) struct ApprovalPreview {
    plan: RemediationPlan,
    identity: HostIdentity,
    attempt: AttemptContext,
}

impl ApprovalPreview {
    pub(crate) fn new(plan: RemediationPlan, attempt: AttemptContext) -> anyhow::Result<Self> {
        Self::new_with_identity(plan, HostIdentity::current()?, attempt, true)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        plan: RemediationPlan,
        identity: HostIdentity,
        attempt: AttemptContext,
    ) -> anyhow::Result<Self> {
        Self::new_with_identity(plan, identity, attempt, false)
    }

    fn new_with_identity(
        plan: RemediationPlan,
        identity: HostIdentity,
        attempt: AttemptContext,
        require_current: bool,
    ) -> anyhow::Result<Self> {
        plan.validate()?;
        identity.validate()?;
        anyhow::ensure!(
            plan.host == identity.host() && plan.os_version == identity.os_version(),
            "remediation plan targets another machine"
        );
        if require_current {
            anyhow::ensure!(
                identity.matches_current()?,
                "host identity changed before consent preview"
            );
        }
        anyhow::ensure!(
            plan.requires_consent,
            "read-only remediation does not require consent"
        );
        anyhow::ensure!(
            plan.prerequisite_id == attempt.task_id(),
            "attempt belongs to another setup task"
        );
        Ok(Self {
            plan,
            identity,
            attempt,
        })
    }

    pub(crate) fn show_confirmation(&self, ui: &mut Ui) -> Option<ApprovalRequest> {
        ui.heading("Review this machine repair");
        ui.label(format!("Operation: {:?}", self.plan.operation));
        ui.label(format!(
            "Affected prerequisite: {}",
            self.plan.prerequisite_id
        ));
        ui.label(format!("Target machine: {}", self.identity.machine_label()));
        ui.label(format!(
            "Target account: {}",
            self.identity.principal_label()
        ));
        ui.label(format!(
            "Identity binding: machine SHA-256 {}; account SHA-256 {}",
            self.identity.machine_fingerprint(),
            self.identity.principal_fingerprint(),
        ));
        ui.label(format!(
            "Platform: {:?} {}",
            self.plan.host, self.plan.os_version
        ));
        ui.label(format!("Privilege: {}", self.plan.privilege.description()));
        ui.label("Effects:");
        for effect in &self.plan.effects {
            ui.label(format!("• {}", effect.description()));
        }
        if !self.plan.file_changes.is_empty() {
            ui.label(format!(
                "File changes: {}",
                self.plan
                    .file_changes
                    .iter()
                    .map(|change| change.description())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else {
            ui.label("Host files changed: none");
        }
        if !self.plan.service_changes.is_empty() {
            ui.label(format!(
                "Service changes: {}",
                self.plan
                    .service_changes
                    .iter()
                    .map(|change| change.description())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else {
            ui.label("Services changed: none");
        }
        if let Some(source) = &self.plan.artifact_source {
            ui.label(format!("Verified source: {}", source.description()));
            let trust = self
                .plan
                .artifact_trust
                .as_ref()
                .map_or_else(|| "none".to_owned(), |value| value.description());
            ui.label(format!("Trust policy: {trust}"));
        }
        ui.label(format!("Recovery: {}", self.plan.rollback.description()));
        ui.label(format!(
            "Readiness checks after this operation: {}",
            self.plan.verification_probe_ids.join(", ")
        ));
        ui.button("Approve this operation")
            .clicked()
            .then(|| ApprovalRequest {
                plan: self.plan.clone(),
                identity: self.identity.clone(),
                attempt: self.attempt.clone(),
            })
    }

    #[cfg(test)]
    pub(crate) fn request_for_test(self) -> ApprovalRequest {
        ApprovalRequest {
            plan: self.plan,
            identity: self.identity,
            attempt: self.attempt,
        }
    }
}

pub(crate) struct ApprovalRequest {
    pub(super) plan: RemediationPlan,
    pub(super) identity: HostIdentity,
    pub(super) attempt: AttemptContext,
}
