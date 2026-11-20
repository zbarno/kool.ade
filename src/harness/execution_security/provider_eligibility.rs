//! Capability-evidence evaluation for contained provider execution (issue #591).
//!
//! Codex and Claude Code are executably containable: the repository ships a
//! pinned, rootless-contained provider image recipe
//! (`containers/provider-cli`, exercised in CI by
//! `containers/provider-cli/tests/run-rootless-containment.sh`) whose
//! security properties (network-none, read-only rootfs, dropped caps,
//! no-new-privileges, bounded pids/memory/cpu, uid-gated tmpfs, single
//! readonly task mount, no engine sockets, credential-free environment) are
//! the threat-model controls for the managed-hook vectors.
//!
//! THIS MODULE ENFORCES CAPABILITY EVIDENCE AT PROVIDER SELECTION: a
//! provider is task-executable only when EVERY precondition below is
//! positively evidenced. Absence, ambiguity, or contradiction of ANY
//! precondition yields an obstruction, and obstructed providers stay
//! blocked. Nothing in this module relaxes the historic unconditional
//! block by default: the evidence-free state evaluates to obstructed, and
//! the legacy block message survives unchanged for the evidence-free path
//! so existing users see zero behavior change until real evidence exists.
//!
//! Preconditions (all conjunctive, all fail-closed):
//! - PLATFORM FACT: a platform fact affirmatively emitted ONLY for
//!   validated Linux amd64/arm64 hosts. Today the fact carries the OS
//!   FAMILY; binding the CPU architecture to the emitted fact is the
//!   evidence-feed slice's standing duty (emit linux facts only for
//!   amd64/arm64 hosts; anything else stays Unknown and obstructs). A
//!   missing or other family is an obstruction — absence of an
//!   obstruction list is never certification.
//! - VERIFIED ROOTLESS ENGINE: the Docker Engine on this host must have
//!   passed rootless + security-option verification (the kind of proof the
//!   `provider_preflight` evaluator and the CI containment scripts emit).
//! - PINNED CONTAINED IMAGE: a digest-pinned image reference for the
//!   provider image exists and matches the digest-only pin grammar
//!   (enforced by `provider_session::validate_image_pin`; an unlisted or
//!   tag-only reference is NOT a pin).
//! - MANAGED-HOOK CONTAINMENT ATTESTED: a recorded, digest-addressable
//!   containment exercise for the synthetic managed-hook class attests the
//!   pinned image exhibits the contained behavior. Unexercised implies
//!   nothing.

use super::CliProvider;
use crate::execution::docker::provider_session::validate_image_pin;

/// One unsatisfied precondition blocking contained execution of a
/// provider. Ordered stably when multiple are collected (see
/// `evaluate_task_execution`), so rendered notices are deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainmentObstruction {
    /// Host platform lacks a validated containment recipe.
    UnsupportedPlatform {
        /// Observed OS family, lowercased identifier (e.g. "macos").
        os: &'static str,
        /// Observed architecture (e.g. "aarch64", "x86_64").
        arch: &'static str,
    },
    /// No verified rootless Engine is available on this host.
    EngineNotVerified,
    /// No digest-pinned contained provider image reference exists.
    ImageNotPinned,
    /// The presented image reference is malformed or not a digest pin.
    InvalidImagePin(String),
    /// No recorded containment-exercise attestation for the pinned image.
    HookContainmentNotAttested,
}

impl ContainmentObstruction {
    /// Canonical human-readable explanation of this obstruction. Surfaces
    /// on dedicated diagnostics channels from the evidence-feed slice;
    /// exercised today by the matrix tests.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "diagnostics surfacing lands with the evidence-feed slice"
        )
    )]
    pub fn describe(&self) -> String {
        match self {
            ContainmentObstruction::UnsupportedPlatform { os, arch } => format!(
                "Provider containment is validated only for Linux amd64/arm64; this host is {os}/{arch}."
            ),
            ContainmentObstruction::EngineNotVerified => "No verified rootless Docker Engine with the required security options is available on this host.".to_string(),
            ContainmentObstruction::ImageNotPinned => "No digest-pinned contained provider image is published for this deployment.".to_string(),
            ContainmentObstruction::InvalidImagePin(reference) => format!(
                "The provider image reference {reference:?} is not a digest-only pin and cannot be admitted."
            ),
            ContainmentObstruction::HookContainmentNotAttested => {
                "No recorded containment-exercise attestation exists for the pinned image.".to_string()
            }
        }
    }
}

/// Positively-evidenced preconditions evaluated as a conjunction. Every
/// field must be affirmative for a runnable verdict; constructing this
/// with holes is a data-preparation problem, not an excuse —
/// `evaluate_task_execution` treats each hole as its obstruction.
#[derive(Debug, Clone, Default)]
pub struct ProviderContainmentEvidence {
    /// Host OS family the deployment targets (identifier spelling from the
    /// probing layer). Positive support requires exactly the validated set.
    pub platform_os: PlatformFact,
    /// Whether the local Engine passed rootless + security verification.
    pub engine_verified: bool,
    /// Digest-pinned provider image facts, if one is published and selected
    /// for this deployment. `None` means "unpinned": `reference` is the
    /// registry reference WITHOUT the digest, `digest` is the standalone
    /// `sha256:<64hex>` token — the same shape the runtime manifest's
    /// pinned-image records carry.
    pub image_pin: Option<ContainedImagePin>,
    /// Whether a recorded containment exercise attests the pinned image.
    pub hook_containment_attested: bool,
}

/// Lowercased OS family identifiers, matching the platform probe spellings.
/// Constructed by production code (the interim evidence feed detects the
/// runtime/build fact) and by tests exercising the other families.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlatformFact {
    Linux,
    Macos,
    Windows,
    #[default]
    Unknown,
}

impl PlatformFact {
    fn is_validated_platform(self) -> bool {
        matches!(self, PlatformFact::Linux)
    }
}

/// Registry reference (digest-free) plus standalone digest token; together
/// they admit exactly one immutable image and nothing else. Constructed
/// from store rows only — never from ad-hoc strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainedImagePin {
    /// Reference WITHOUT digest, e.g. `ghcr.io/org/koolade-provider-cli`.
    pub reference: String,
    /// Exactly `sha256:` + 64 lowercase hex.
    pub digest: String,
}

/// Evaluate whether `provider` may execute tasks under contained execution.
/// Fail-closed: collect EVERY obstruction (stable order) rather than
/// short-circuiting, so operators see the full recovery picture at once.
pub fn evaluate_task_execution(
    provider: CliProvider,
    evidence: &ProviderContainmentEvidence,
) -> TaskExecutionVerdict {
    // Providers without host-side managed-hook exposure are assessed by the
    // normal profile machinery; this evaluator only governs the
    // managed-hook-bearing CLIs. They pass through unchanged.
    if !matches!(provider, CliProvider::Codex | CliProvider::Claude) {
        return TaskExecutionVerdict::OutOfScope;
    }
    let mut obstructions = Vec::new();
    if !evidence.platform_os.is_validated_platform() {
        obstructions.push(obstruction_for_platform(evidence.platform_os));
    }
    if !evidence.engine_verified {
        obstructions.push(ContainmentObstruction::EngineNotVerified);
    }
    match &evidence.image_pin {
        None => obstructions.push(ContainmentObstruction::ImageNotPinned),
        Some(pin) => {
            if let Err(error) = validate_image_pin(&pin.reference, &pin.digest) {
                obstructions.push(ContainmentObstruction::InvalidImagePin(format!(
                    "{}:{}: {error}",
                    pin.reference, pin.digest
                )));
            }
        }
    }
    if !evidence.hook_containment_attested {
        obstructions.push(ContainmentObstruction::HookContainmentNotAttested);
    }
    if obstructions.is_empty() {
        TaskExecutionVerdict::Runnable
    } else {
        TaskExecutionVerdict::Obstructed(obstructions)
    }
}

fn obstruction_for_platform(os: PlatformFact) -> ContainmentObstruction {
    let os_label = match os {
        PlatformFact::Linux => "linux",
        PlatformFact::Macos => "macos",
        PlatformFact::Windows => "windows",
        PlatformFact::Unknown => "unidentified",
    };
    ContainmentObstruction::UnsupportedPlatform {
        os: os_label,
        arch: std::env::consts::ARCH,
    }
}

/// Outcome of the conjunction evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskExecutionVerdict {
    /// The provider has no managed-hook exposure governed by this
    /// evaluator; ordinary profile rules apply.
    OutOfScope,
    /// Every precondition is positively evidenced.
    Runnable,
    /// One or more preconditions are unmet (ordered collection).
    Obstructed(Vec<ContainmentObstruction>),
}

#[cfg(test)]
#[path = "provider_eligibility/tests.rs"]
mod tests;
