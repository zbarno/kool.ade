//! Signed app-bundled runtime manifest (v1).
//!
//! Authenticates prebuilt host runtime artifacts — today the per-architecture
//! QEMU executable the macOS adapter launches, later the Windows equivalents
//! and bundled disk images. This manifest is the trust anchor declared by the
//! macOS QEMU binary receipt (`MacOsTrustedQemuRuntimeV1`): the expected
//! digest for a bundled executable must originate here, nowhere else.
//!
//! Verification is fail-closed: a missing, unreadable, oversized, stale,
//! malformed, or incorrectly signed manifest never yields a verified
//! artifact. Carrying a verified artifact does NOT certify a platform: host
//! adapters still enforce their own storage, process, release-gate, and
//! isolation controls before launching anything this manifest references.

mod application_trust;
mod bundle;
mod encoding;
mod manifest;
mod verification;

#[cfg(test)]
#[path = "runtime_manifest/tests.rs"]
mod tests;

pub use application_trust::application_runtime_manifest_trust_v1;
pub use bundle::{BUNDLED_RUNTIME_MANIFEST_FILENAME, load_bundled_runtime_manifest};
pub use manifest::{
    MAX_RUNTIME_ARTIFACT_ENTRIES, MAX_RUNTIME_MANIFEST_BYTES, RUNTIME_MANIFEST_SCHEMA_VERSION,
    RuntimeArtifactKindV1, RuntimeArtifactV1, RuntimeManifestErrorV1, RuntimeManifestPlatformV1,
    RuntimeManifestTrustV1, RuntimeManifestV1,
};
pub use verification::{
    VerifiedRuntimeArtifactV1, VerifiedRuntimeManifestV1, verify_runtime_manifest,
};
