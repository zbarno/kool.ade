use std::{cell::RefCell, collections::BTreeSet};

use egui::Ui;

use super::super::{
    attempt::AttemptContext,
    identity::HostIdentity,
    plan::{PlanConsent, RemediationPlan},
};

pub(crate) struct ApprovalPreview {
    plan: RemediationPlan,
    identity: HostIdentity,
    attempt: AttemptContext,
    selected_consents: RefCell<BTreeSet<PlanConsent>>,
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
            selected_consents: RefCell::new(BTreeSet::new()),
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
        if !self.plan.precondition_probe_ids.is_empty() {
            ui.label("Required checks before this operation:");
            for probe in &self.plan.precondition_probe_ids {
                ui.label(format!("• {probe}"));
            }
        }
        ui.label("Effects:");
        for effect in &self.plan.effects {
            ui.label(format!("• {}", effect.description()));
        }
        if !self.plan.package_changes.is_empty() {
            ui.label(format!(
                "Package changes: {}",
                self.plan
                    .package_changes
                    .iter()
                    .map(|change| change.description())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else {
            ui.label("Packages changed: none");
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
        if !self.plan.required_consents.is_empty() {
            ui.heading("Additional consent required");
            for consent in &self.plan.required_consents {
                let mut selected = self.selected_consents.borrow_mut();
                let mut accepted = selected.contains(consent);
                if ui.checkbox(&mut accepted, consent.description()).changed() {
                    if accepted {
                        selected.insert(*consent);
                    } else {
                        selected.remove(consent);
                    }
                }
            }
        }
        let consented = all_required_consents_selected(
            &self.plan.required_consents,
            &self.selected_consents.borrow(),
        );
        ui.add_enabled(consented, egui::Button::new("Approve this operation"))
            .clicked()
            .then(|| ApprovalRequest {
                plan: self.plan.clone(),
                identity: self.identity.clone(),
                attempt: self.attempt.clone(),
                consented: self.plan.required_consents.clone(),
            })
    }

    #[cfg(test)]
    pub(crate) fn request_for_test(self) -> ApprovalRequest {
        ApprovalRequest {
            consented: self.plan.required_consents.clone(),
            plan: self.plan,
            identity: self.identity,
            attempt: self.attempt,
        }
    }
}

fn all_required_consents_selected(
    required: &[PlanConsent],
    selected: &BTreeSet<PlanConsent>,
) -> bool {
    required.iter().all(|consent| selected.contains(consent))
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;

pub(crate) struct ApprovalRequest {
    pub(in crate::app::bootstrap_remediation) plan: RemediationPlan,
    pub(in crate::app::bootstrap_remediation) identity: HostIdentity,
    pub(in crate::app::bootstrap_remediation) attempt: AttemptContext,
    pub(in crate::app::bootstrap_remediation) consented: Vec<PlanConsent>,
}
