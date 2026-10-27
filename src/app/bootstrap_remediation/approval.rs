use std::{collections::HashMap, time::SystemTime};

use uuid::Uuid;

use crate::{
    app::bootstrap_remediation::{
        attempt::AttemptContext, identity::HostIdentity, plan::RemediationPlan,
    },
    persistence::setup_tasks::AuthorizationBindingV1,
};

mod preview;
pub(crate) use preview::{ApprovalPreview, ApprovalRequest};

const RECEIPT_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Default)]
pub(crate) struct ApprovalGate {
    pending: HashMap<String, Pending>,
}

struct Pending {
    plan_digest: String,
    host_digest: String,
    scope_digest: String,
    attempt: AttemptContext,
    expires_at_unix_ms: u64,
}

impl ApprovalGate {
    pub(crate) fn grant(
        &mut self,
        request: ApprovalRequest,
    ) -> anyhow::Result<AuthorizationBindingV1> {
        request.plan.validate_current_target()?;
        let current_identity = HostIdentity::current()?;
        self.grant_inner(request, &current_identity, now_unix_ms()?)
    }

    #[cfg(test)]
    pub(crate) fn grant_at(
        &mut self,
        request: ApprovalRequest,
        current_identity: &HostIdentity,
        now_unix_ms: u64,
    ) -> anyhow::Result<AuthorizationBindingV1> {
        self.grant_inner(request, current_identity, now_unix_ms)
    }

    fn grant_inner(
        &mut self,
        request: ApprovalRequest,
        current_identity: &HostIdentity,
        now_unix_ms: u64,
    ) -> anyhow::Result<AuthorizationBindingV1> {
        request.plan.validate()?;
        current_identity.validate()?;
        anyhow::ensure!(
            request.consented == request.plan.required_consents,
            "required machine repair consent was not granted"
        );
        let plan_digest = request.plan.digest()?;
        let host_digest = request.identity.digest()?;
        anyhow::ensure!(
            request.plan.host == request.identity.host()
                && request.plan.os_version == request.identity.os_version()
                && current_identity.host() == request.identity.host()
                && current_identity.os_version() == request.identity.os_version()
                && request.attempt.task_id() == request.plan.prerequisite_id,
            "approval preview no longer matches its host or setup task"
        );
        anyhow::ensure!(
            current_identity.digest()? == host_digest,
            "machine or user identity changed while approval was open"
        );
        anyhow::ensure!(
            request.plan.requires_consent,
            "operation does not require approval"
        );
        anyhow::ensure!(
            request.attempt.scope_digest()
                == expected_scope_digest(&request.plan, &request.identity, &request.attempt)?,
            "approval attempt does not bind this plan and host"
        );
        let receipt_id = Uuid::new_v4().to_string();
        let binding = AuthorizationBindingV1 {
            receipt_id: receipt_id.clone(),
            scope_digest: request.attempt.scope_digest().to_owned(),
            authorized_at_unix_ms: now_unix_ms,
        };
        self.pending.insert(
            receipt_id,
            Pending {
                plan_digest,
                host_digest,
                scope_digest: binding.scope_digest.clone(),
                attempt: request.attempt,
                expires_at_unix_ms: now_unix_ms.saturating_add(RECEIPT_TTL_MS),
            },
        );
        Ok(binding)
    }

    pub(crate) fn consume(
        &mut self,
        binding: &AuthorizationBindingV1,
        plan: &RemediationPlan,
        attempt: &AttemptContext,
    ) -> anyhow::Result<ApprovedRemediation> {
        plan.validate_current_target()?;
        let current_identity = HostIdentity::current()?;
        self.consume_inner(binding, plan, &current_identity, attempt, now_unix_ms()?)
    }

    #[cfg(test)]
    pub(crate) fn consume_at(
        &mut self,
        binding: &AuthorizationBindingV1,
        plan: &RemediationPlan,
        current_identity: &HostIdentity,
        attempt: &AttemptContext,
        now_unix_ms: u64,
    ) -> anyhow::Result<ApprovedRemediation> {
        self.consume_inner(binding, plan, current_identity, attempt, now_unix_ms)
    }

    fn consume_inner(
        &mut self,
        binding: &AuthorizationBindingV1,
        plan: &RemediationPlan,
        current_identity: &HostIdentity,
        attempt: &AttemptContext,
        now_unix_ms: u64,
    ) -> anyhow::Result<ApprovedRemediation> {
        let pending = self
            .pending
            .remove(&binding.receipt_id)
            .ok_or_else(|| anyhow::anyhow!("remediation approval is missing or already used"))?;
        plan.validate()?;
        current_identity.validate()?;
        let plan_digest = plan.digest()?;
        let host_digest = current_identity.digest()?;
        let expected_scope = expected_scope_digest(plan, current_identity, attempt)?;
        let valid = now_unix_ms <= pending.expires_at_unix_ms
            && pending.plan_digest == plan_digest
            && pending.host_digest == host_digest
            && pending.scope_digest == binding.scope_digest
            && binding.scope_digest == expected_scope
            && pending.attempt == *attempt
            && plan.requires_consent
            && plan.host == current_identity.host()
            && plan.os_version == current_identity.os_version();
        anyhow::ensure!(
            valid,
            "remediation consent expired or no longer matches the approved operation"
        );
        Ok(ApprovedRemediation {
            plan_digest,
            host_digest,
            scope_digest: binding.scope_digest.clone(),
            receipt_id: binding.receipt_id.clone(),
            task_id: attempt.task_id().to_owned(),
            operation_id: attempt.operation_id().to_owned(),
            owner_instance_id: attempt.owner_instance_id().to_owned(),
            expires_at_unix_ms: pending.expires_at_unix_ms,
        })
    }

    pub(crate) fn cancel(&mut self, receipt_id: &str) {
        self.pending.remove(receipt_id);
    }
}

pub(crate) struct ApprovedRemediation {
    pub(super) plan_digest: String,
    pub(super) host_digest: String,
    pub(super) scope_digest: String,
    pub(super) receipt_id: String,
    pub(super) task_id: String,
    pub(super) operation_id: String,
    pub(super) owner_instance_id: String,
    pub(super) expires_at_unix_ms: u64,
}

impl ApprovedRemediation {
    pub(super) fn matches(
        &self,
        plan: &RemediationPlan,
        identity: &HostIdentity,
    ) -> anyhow::Result<bool> {
        Ok(self.plan_digest == plan.digest()? && self.host_digest == identity.digest()?)
    }
}

fn expected_scope_digest(
    plan: &RemediationPlan,
    identity: &HostIdentity,
    attempt: &AttemptContext,
) -> anyhow::Result<String> {
    let scopes = super::attempt::bound_scopes(plan, identity)?;
    anyhow::ensure!(
        scopes == attempt.requested_scopes(),
        "remediation attempt scopes changed"
    );
    Ok(crate::persistence::setup_tasks::authorization_scope_digest(
        attempt.task_id(),
        attempt.operation_id(),
        plan.operation_code_version,
        &scopes,
    ))
}

pub(crate) fn now_unix_ms() -> anyhow::Result<u64> {
    Ok(SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_millis() as u64)
}
