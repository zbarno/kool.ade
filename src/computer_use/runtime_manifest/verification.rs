use ed25519_dalek::{Signature, Verifier};

use super::{
    encoding,
    manifest::{
        MAX_RUNTIME_MANIFEST_BYTES, RuntimeArtifactV1, RuntimeManifestErrorV1,
        RuntimeManifestPlatformV1, RuntimeManifestTrustV1, RuntimeManifestV1,
    },
};

/// A manifest whose envelope and every entry passed verification at a
/// specific instant of host time.
#[derive(Clone, Debug)]
pub struct VerifiedRuntimeManifestV1 {
    manifest: RuntimeManifestV1,
    digest: String,
    signer_key_id: String,
    generation: u64,
    expires_at_unix_ms: u64,
}

impl VerifiedRuntimeManifestV1 {
    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn release_version(&self) -> &str {
        &self.manifest.release_version
    }

    pub fn signer_key_id(&self) -> &str {
        &self.signer_key_id
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }

    pub fn artifacts(&self) -> impl Iterator<Item = VerifiedRuntimeArtifactV1> + '_ {
        self.manifest
            .artifacts
            .iter()
            .map(VerifiedRuntimeArtifactV1::bind)
    }

    /// Locate the QEMU executable this manifest authenticates for `platform`.
    /// Selection is exact equality on the canonical bundle path; a platform
    /// whose adapter has not adopted this manifest has no canonical path and
    /// therefore no artifact here.
    pub fn qemu_executable_for(
        &self,
        platform: RuntimeManifestPlatformV1,
    ) -> Option<VerifiedRuntimeArtifactV1> {
        let expected = platform.canonical_qemu_executable_path();
        if expected.is_empty() {
            return None;
        }
        self.manifest
            .artifacts
            .iter()
            .find(|artifact| {
                artifact.kind == super::manifest::RuntimeArtifactKindV1::Executable
                    && artifact.platform == platform
                    && artifact.bundle_relative_path == expected
            })
            .map(VerifiedRuntimeArtifactV1::bind)
    }
}

/// A single manifest entry, projected and immutably bound.
#[derive(Clone, Debug)]
pub struct VerifiedRuntimeArtifactV1(RuntimeArtifactV1);

impl VerifiedRuntimeArtifactV1 {
    pub(super) fn bind(artifact: &RuntimeArtifactV1) -> Self {
        Self(artifact.clone())
    }

    pub fn artifact_id(&self) -> &str {
        &self.0.artifact_id
    }

    pub fn kind(&self) -> super::manifest::RuntimeArtifactKindV1 {
        self.0.kind
    }

    pub fn platform(&self) -> RuntimeManifestPlatformV1 {
        self.0.platform
    }

    pub fn guest_architecture(
        &self,
    ) -> crate::computer_use::guest_image::ComputerGuestArchitectureV1 {
        self.0.guest_architecture
    }

    pub fn bundle_relative_path(&self) -> &str {
        &self.0.bundle_relative_path
    }

    /// Digest in the exact `sha256:<lowercase-hex>` spelling the host
    /// receipts consume.
    pub fn digest(&self) -> &str {
        &self.0.sha256
    }

    pub fn size_bytes(&self) -> u64 {
        self.0.size_bytes
    }

    pub fn permission_bits(&self) -> u32 {
        self.0.permission_bits
    }
}

/// Verify a signed runtime manifest envelope under app-pinned trust.
///
/// Every failure is fail-closed: the caller must treat an error as "no
/// verified artifact exists" and must never fall back to PATH lookups,
/// embedded guesses, or operator-supplied digests.
pub fn verify_runtime_manifest(
    bytes: &[u8],
    trust: &RuntimeManifestTrustV1,
    now_unix_ms: u64,
) -> Result<VerifiedRuntimeManifestV1, RuntimeManifestErrorV1> {
    if bytes.len() > MAX_RUNTIME_MANIFEST_BYTES {
        return Err(RuntimeManifestErrorV1::TooLarge);
    }
    let manifest: RuntimeManifestV1 =
        serde_json::from_slice(bytes).map_err(|_| RuntimeManifestErrorV1::DecodeFailed)?;
    manifest.validate()?;
    // A leaked envelope dated into the future is refused outright, mirroring
    // the guest-image catalog's issuance rule.
    if manifest.issued_at_unix_ms > now_unix_ms {
        return Err(RuntimeManifestErrorV1::InvalidHeader);
    }
    if now_unix_ms >= manifest.expires_at_unix_ms {
        return Err(RuntimeManifestErrorV1::Expired);
    }
    if manifest.generation < trust.minimum_generation() {
        return Err(RuntimeManifestErrorV1::BelowGenerationFloor);
    }
    let key = trust
        .key(&manifest.signer_key_id)
        .ok_or(RuntimeManifestErrorV1::UnknownSigner)?;
    let signature = Signature::from_slice(
        &encoding::decode_lower_hex(&manifest.signature_hex)
            .map_err(|_| RuntimeManifestErrorV1::BadSignature)?,
    )
    .map_err(|_| RuntimeManifestErrorV1::BadSignature)?;
    key.verify(&encoding::signing_payload_bytes(&manifest)?, &signature)
        .map_err(|_| RuntimeManifestErrorV1::BadSignature)?;
    Ok(VerifiedRuntimeManifestV1 {
        generation: manifest.generation,
        expires_at_unix_ms: manifest.expires_at_unix_ms,
        signer_key_id: manifest.signer_key_id.clone(),
        digest: manifest.digest()?,
        manifest,
    })
}
