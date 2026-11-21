//! Interim capability-evidence feed (issue #591 phase 4, piece 3).
//!
//! Populates [`super::provider_eligibility::ProviderContainmentEvidence`]
//! from REAL sources so the selection path can finally distinguish
//! "evidence exists" from "evidence does not":
//!
//! - **Platform fact**: from the build/runtime OS plus a compile-time
//!   architecture gate (the arch-binding duty stated by the eligibility
//!   module): non-Linux or exotic architectures yield `Unknown`.
//! - **Image pin**: extracted from the COMPILED-IN `containers/
//!   images.lock.json` (the same bytes the policy fingerprint digests)
//!   under the additive `profiles.provider_cli` key. The key is absent in
//!   every released lock today, so every current deployment gathers
//!   `image_pin = None` and cannot use the contained route — enabling that route requires
//!   operators to publish a pinned, validated provider-runtime profile entry
//!   through the image promotion pipeline. Malformed entries fail CLOSED
//!   to absence (garbage never admits).
//! - **Engine verification / hook containment attestation**: interim
//!   OPERATOR-ASSERTED env switches
//!   (`KOOLADE_PROVIDER_ENGINE_VERIFIED=1`,
//!   `KOOLADE_PROVIDER_HOOK_CONTAINMENT_ATTESTED=1`). This is a narrow,
//!   loud, default-OFF bring-up channel: it raises eligibility ONLY —
//!   containment itself still comes exclusively from the session layer
//!   (sealed specs, verified teardown). Pipeline-grade verifiers REPLACE
//!   these switches in a later slice (standing obligation, design doc).
//!
//! Delivery into selection: a once-initialized process
//! ([`snapshot`]) consumed by the Claude Worker profile. Codex currently
//! uses the host CLI and does not claim containment; see issue #725. Tests use
//! the pure [`gather_provider_evidence`] with injected inputs instead of
//! env/global state.

use std::sync::{Arc, OnceLock};

use serde_json::Value;

use super::provider_eligibility::{ContainedImagePin, PlatformFact};
use crate::execution::docker::provider_session::validate_image_pin;

/// Additive lock key under `profiles` carrying the provider-runtime image
/// entries. Released locks lack the key => absence => obstructed.
pub const PROVIDER_CLI_LOCK_PROFILE_KEY: &str = "provider_cli";

/// Compiled-in release pin lock (fingerprint-parity with policy hashing).
const IMAGE_LOCK_JSON: &[u8] = include_bytes!("../../../containers/images.lock.json");

/// Injection inputs for [`gather_provider_evidence`]; the pure seam tests
/// exercise directly (no env, no globals).
#[derive(Debug, Clone, Copy, Default)]
pub struct ProviderEvidenceInputs<'a> {
    /// Raw bytes of the release image lock.
    pub lock_bytes: &'a [u8],
    /// Engine-verified facet (production: interim env switch).
    pub engine_verified: bool,
    /// Hook-containment-attested facet (production: interim env switch).
    pub hook_containment_attested: bool,
    /// Platform fact to record (production: build/runtime detection).
    pub platform_os: PlatformFact,
}

/// Maps the target triple's architecture word to the lock's platform word.
fn target_platform_word() -> Option<&'static str> {
    if cfg!(target_arch = "x86_64") {
        Some("linux/amd64")
    } else if cfg!(target_arch = "aarch64") {
        Some("linux/arm64")
    } else {
        None
    }
}

/// Pure platform detection: runtime OS family + compile-time arch gate.
/// Outside linux/amd64|arm64 the fact degrades to `Unknown` and the
/// eligibility conjunction stays obstructed (arch-binding duty).
pub fn detect_platform_fact() -> PlatformFact {
    let fam = if std::env::consts::OS == "linux" {
        PlatformFact::Linux
    } else if std::env::consts::OS == "macos" {
        PlatformFact::Macos
    } else if std::env::consts::OS == "windows" {
        PlatformFact::Windows
    } else {
        PlatformFact::Unknown
    };
    match (fam, target_platform_word().is_none()) {
        (PlatformFact::Linux, false) => PlatformFact::Linux,
        _ => PlatformFact::Unknown,
    }
}

/// Extracts the provider-runtime pin from a release image lock.
///
/// Shape consumed: `{"profiles":{"provider_cli":[{"platform":"linux/amd64"|
/// "linux/arm64","reference":"…@sha256:…","digest":"sha256:…"}…]}}`.
/// Absent key/profile/entry => `Ok(None)` (deployments without a published
/// pin stay obstructed). Any MALFORMED candidate fails closed to
/// `Ok(None)`: only an entry passing the digest-only pin grammar admits.
pub fn extract_provider_image_pin(lock_bytes: &[u8]) -> Option<ContainedImagePin> {
    let wanted = target_platform_word()?;
    let lock: Value = serde_json::from_slice(lock_bytes).ok()?;
    let profiles = lock.get("profiles")?.get(PROVIDER_CLI_LOCK_PROFILE_KEY)?;
    let entries = profiles.as_array()?;
    for entry in entries {
        let platform = entry.get("platform")?.as_str()?;
        if platform != wanted {
            continue;
        }
        let reference = entry.get("reference")?.as_str()?.to_string();
        let digest = entry.get("digest")?.as_str()?.to_string();
        // Grammar gate (digest-only, canonical, no local registries) is the
        // single authority shared with the launch path.
        if validate_image_pin(&reference, &digest).is_ok() {
            return Some(ContainedImagePin { reference, digest });
        }
        // Fall through: this candidate is unusable; try the next (there
        // should be at most one matching platform; none admit on failure).
    }
    None
}

/// Pure assembly of the evidence context from injectable inputs.
pub fn gather_provider_evidence(
    inputs: ProviderEvidenceInputs,
) -> super::provider_eligibility::ProviderContainmentEvidence {
    super::provider_eligibility::ProviderContainmentEvidence {
        platform_os: inputs.platform_os,
        engine_verified: inputs.engine_verified,
        image_pin: extract_provider_image_pin(inputs.lock_bytes),
        hook_containment_attested: inputs.hook_containment_attested,
    }
}

fn interim_assert_switch(var: &str) -> bool {
    std::env::var(var) == Ok("1".to_string())
}

fn production_inputs() -> ProviderEvidenceInputs<'static> {
    ProviderEvidenceInputs {
        lock_bytes: IMAGE_LOCK_JSON,
        engine_verified: interim_assert_switch("KOOLADE_PROVIDER_ENGINE_VERIFIED"),
        hook_containment_attested: interim_assert_switch(
            "KOOLADE_PROVIDER_HOOK_CONTAINMENT_ATTESTED",
        ),
        platform_os: detect_platform_fact(),
    }
}

/// Process-wide evidence snapshot: first reader gathers from production
/// inputs and freezes; every later reader sees the same Arc. With nothing
/// provisioned (the universal current state) this IS the default-obstructed
/// context, so selection behavior stays byte-for-byte historic.
pub fn snapshot() -> Arc<super::provider_eligibility::ProviderContainmentEvidence> {
    static SNAPSHOT: OnceLock<Arc<super::provider_eligibility::ProviderContainmentEvidence>> =
        OnceLock::new();
    Arc::clone(SNAPSHOT.get_or_init(|| Arc::new(gather_provider_evidence(production_inputs()))))
}

#[cfg(test)]
#[path = "provider_evidence_feed/tests.rs"]
mod tests;
