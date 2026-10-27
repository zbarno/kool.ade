mod artifact;
mod catalog;
mod invocation;

#[cfg(test)]
mod tests;

pub(crate) use artifact::{PublisherSignatureVerifier, VerifiedArtifact, verify_plan_artifact};
pub(crate) use catalog::TrustedProgram;
#[cfg(target_os = "linux")]
pub(crate) use invocation::ClaudeReleaseAsset;
#[cfg(target_os = "linux")]
pub(crate) use invocation::InternalInvocation;
pub(crate) use invocation::{
    DNF_MAX_STAGED_RPMS, DnfPackageNames, DnfStageId, HashBoundDnfRpm, StagedRpm, TrustedArgument,
    TrustedEnvironment, TrustedInvocation, VerifiedDnfRpm, parse_installed_package_names,
};
