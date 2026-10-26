//! Typed, consent-bound broker for future host remediation operations.
//!
//! This module exposes no model-controlled executable or shell interface.

mod approval;
mod attempt;
mod identity;
mod plan;
mod redaction;
mod request;
mod supervisor;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub(crate) use approval::{ApprovalGate, ApprovalPreview, ApprovalRequest, ApprovedRemediation};
#[allow(unused_imports)]
pub(crate) use attempt::AttemptContext;
#[allow(unused_imports)]
pub(crate) use identity::HostIdentity;
#[allow(unused_imports)]
pub(crate) use plan::{ArtifactTrust, PrivilegeLevel, RemediationPlan, RollbackGuidance};
pub(crate) use redaction::redact_keyed_values;
#[allow(unused_imports)]
pub(crate) use request::{OperationParameters, RemediationRequest};
#[allow(unused_imports)]
pub(crate) use supervisor::{
    Cancellation, ElevationReceipt, NativeCommandRunner, NativeElevation, OperationRegistry,
    ProcessOutput, ProcessRunError, ProcessRunFailure, PublisherSignatureVerifier,
    ReadinessVerifier, RemediationResult, RemediationSupervisor, SystemCommandRunner,
    TrustedArgument, TrustedInvocation, TrustedProgram, VerificationReceipt, VerifiedArtifact,
    verify_plan_artifact,
};
