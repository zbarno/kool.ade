mod artifact;
mod catalog;
mod invocation;

#[cfg(test)]
mod tests;

pub(crate) use artifact::{PublisherSignatureVerifier, VerifiedArtifact, verify_plan_artifact};
pub(crate) use catalog::TrustedProgram;
#[cfg(target_os = "linux")]
pub(crate) use invocation::ClaudeReleaseAsset;
pub(crate) use invocation::{InternalInvocation, TrustedArgument, TrustedInvocation};
