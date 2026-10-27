//! Typed, consent-bound broker for future host remediation operations.
//!
//! This module exposes no model-controlled executable or shell interface.

mod approval;
mod attempt;
mod controller;
mod identity;
mod plan;
mod production;
mod redaction;
mod request;
mod supervisor;
mod trusted_assets;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub(crate) use approval::{ApprovalGate, ApprovalPreview, ApprovalRequest, ApprovedRemediation};
#[allow(unused_imports)]
pub(crate) use attempt::AttemptContext;
pub(crate) use controller::Controller;
#[allow(unused_imports)]
pub(crate) use identity::HostIdentity;
#[allow(unused_imports)]
pub(crate) use plan::{ArtifactTrust, PrivilegeLevel, RemediationPlan, RollbackGuidance};
#[allow(unused_imports)]
pub(crate) use production::{
    PackageSourceArtifacts, ProductionOperationRegistry, package_install_arguments,
    package_source_artifacts, production_package_source_ready, validate_before_elevated_launch,
    validate_current_mappings_for_uid,
};
pub(crate) use redaction::redact_keyed_values;
#[allow(unused_imports)]
pub(crate) use request::{OperationParameters, RemediationRequest};
#[allow(unused_imports)]
pub(crate) use supervisor::{
    Cancellation, DNF_MAX_STAGED_RPMS, DnfPackageNames, DnfStageId, ElevationReceipt,
    HashBoundDnfRpm, LinuxNativeElevation, LinuxReadinessVerifier, NativeCommandRunner,
    NativeElevation, OperationRegistry, PreconditionReceipt, PreconditionVerifier, ProcessOutput,
    ProcessRunError, ProcessRunFailure, PublisherSignatureVerifier, ReadinessVerifier,
    RemediationResult, RemediationSupervisor, StagedRpm, SystemCommandRunner, TrustedArgument,
    TrustedInvocation, TrustedProgram, VerificationReceipt, VerifiedArtifact, VerifiedDnfRpm,
    parse_installed_package_names, verify_plan_artifact,
};
