use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::bootstrap_remediation::{
    identity::HostIdentity,
    plan::{PrivilegeLevel, RemediationPlan},
};

mod execute;
mod process;

pub(crate) use execute::RemediationSupervisor;
pub(crate) use process::{
    NativeCommandRunner, ProcessOutput, ProcessRunError, ProcessRunFailure,
    PublisherSignatureVerifier, SystemCommandRunner, TrustedArgument, TrustedInvocation,
    TrustedProgram, VerifiedArtifact, verify_plan_artifact,
};

pub(crate) struct Cancellation(AtomicBool);

impl Cancellation {
    pub(crate) fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

pub(crate) trait OperationRegistry {
    fn compile(&self, plan: &RemediationPlan) -> anyhow::Result<TrustedInvocation>;
}

pub(crate) trait NativeElevation {
    /// Return `None` when the operator declines the OS-native privilege prompt.
    fn request(
        &self,
        plan: &RemediationPlan,
        identity: &HostIdentity,
        invocation: &TrustedInvocation,
        cancellation: &Cancellation,
    ) -> anyhow::Result<Option<ElevationReceipt>>;
}

pub(crate) struct ElevationReceipt {
    plan_digest: String,
    host_digest: String,
    invocation_digest: String,
}

impl ElevationReceipt {
    fn issued_after_native_prompt(
        plan: &RemediationPlan,
        identity: &HostIdentity,
        invocation: &TrustedInvocation,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            plan.privilege == PrivilegeLevel::Administrator,
            "native elevation is not justified for this operation"
        );
        anyhow::ensure!(
            plan.host == identity.host() && plan.os_version == identity.os_version(),
            "native elevation targets another host OS"
        );
        Ok(Self {
            plan_digest: plan.digest()?,
            host_digest: identity.digest()?,
            invocation_digest: invocation.binding_digest()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        plan: &RemediationPlan,
        identity: &HostIdentity,
        invocation: &TrustedInvocation,
    ) -> anyhow::Result<Self> {
        Self::issued_after_native_prompt(plan, identity, invocation)
    }

    pub(super) fn matches(
        &self,
        plan: &RemediationPlan,
        identity: &HostIdentity,
        invocation: &TrustedInvocation,
    ) -> anyhow::Result<bool> {
        Ok(self.plan_digest == plan.digest()?
            && self.host_digest == identity.digest()?
            && self.invocation_digest == invocation.binding_digest()?)
    }
}

pub(crate) trait ReadinessVerifier {
    fn verify(&self, plan: &RemediationPlan) -> anyhow::Result<VerificationReceipt>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerificationReceipt {
    pub(super) probe_ids: Vec<String>,
    pub(super) observed_at_unix_ms: u64,
    pub(super) evidence_digest: String,
    pub(super) ready: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RemediationResult {
    pub(super) exit_code: i32,
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) stdout_truncated: bool,
    pub(super) stderr_truncated: bool,
    pub(super) verification: VerificationReceipt,
}
