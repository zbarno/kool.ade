use std::collections::{BTreeMap, BTreeSet};

mod encoding;

#[cfg(test)]
mod tests;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

use self::encoding::{
    canonical_manifest_bytes, decode_hex, is_git_revision, is_release_tag, is_sha256, is_uuid,
    sha256,
};
use super::ImageCacheErrorV1;
use crate::computer_use::{ComputerSessionClassV1, ComputerSessionV1};

pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_IMAGE_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerGuestArchitectureV1 {
    X86_64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestImageStatusV1 {
    Approved,
    Revoked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct GuestImageEntryV1 {
    pub architecture: ComputerGuestArchitectureV1,
    pub image_digest: String,
    pub image_size_bytes: u64,
    pub artifact_size_bytes: u64,
    pub artifact_name: String,
    pub guest_release: String,
    pub source_revision: String,
    pub build_recipe_sha256: String,
    pub sbom_sha256: String,
    pub build_attestation_sha256: String,
    pub companion_protocol_version: u16,
    pub companion_sha256: String,
    pub status: GuestImageStatusV1,
}

impl GuestImageEntryV1 {
    fn validate(&self) -> Result<(), ImageCacheErrorV1> {
        if !is_sha256(&self.image_digest)
            || !is_sha256(&self.build_recipe_sha256)
            || !is_sha256(&self.sbom_sha256)
            || !is_sha256(&self.build_attestation_sha256)
            || !is_sha256(&self.companion_sha256)
        {
            return Err(ImageCacheErrorV1::InvalidManifest("invalid digest"));
        }
        if self.artifact_size_bytes == 0
            || self.artifact_size_bytes > MAX_IMAGE_ARTIFACT_BYTES
            || self.image_size_bytes < 2 * 1024 * 1024 * 1024
            || self.image_size_bytes > MAX_IMAGE_ARTIFACT_BYTES
        {
            return Err(ImageCacheErrorV1::InvalidManifest(
                "image size outside bounds",
            ));
        }
        if self.companion_protocol_version == 0
            || self.artifact_name != "koolade-computer-use-guest-x86_64.qcow2"
            || !is_git_revision(&self.source_revision)
            || self.guest_release.trim().is_empty()
            || self.guest_release.len() > 80
            || !self
                .guest_release
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
        {
            return Err(ImageCacheErrorV1::InvalidManifest(
                "invalid artifact metadata",
            ));
        }
        Ok(())
    }
}

/// Language-neutral signed catalog shipped by an app release.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GuestImageManifestV1 {
    pub schema_version: u16,
    pub catalog_version: String,
    pub catalog_generation: u64,
    pub issued_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub signer_key_id: String,
    pub images: Vec<GuestImageEntryV1>,
    pub signature_hex: String,
}

#[derive(Clone, Debug, Default)]
pub struct GuestImageManifestTrustV1 {
    /// Must contain keys pinned by the signed application distribution.
    keys: BTreeMap<String, VerifyingKey>,
    minimum_catalog_generation: u64,
}

impl GuestImageManifestTrustV1 {
    /// The key list must come from app-pinned trust; the generation floor is
    /// durable host state advanced after each successful catalog verification.
    pub fn from_application_pinned_keys(
        keys: impl IntoIterator<Item = (String, VerifyingKey)>,
        minimum_catalog_generation: u64,
    ) -> Result<Self, ImageCacheErrorV1> {
        let mut trusted = BTreeMap::new();
        for (key_id, key) in keys {
            if !is_uuid(&key_id) || trusted.insert(key_id, key).is_some() {
                return Err(ImageCacheErrorV1::InvalidTrustStore);
            }
        }
        if trusted.is_empty() || minimum_catalog_generation == 0 {
            return Err(ImageCacheErrorV1::InvalidTrustStore);
        }
        Ok(Self {
            keys: trusted,
            minimum_catalog_generation,
        })
    }

    fn key(&self, id: &str) -> Option<&VerifyingKey> {
        self.keys.get(id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedGuestImageV1 {
    pub(crate) entry: GuestImageEntryV1,
    pub(crate) manifest_digest: String,
    pub(crate) manifest_signer_key_id: String,
    pub(crate) catalog_version: String,
    pub(crate) catalog_generation: u64,
}

impl VerifiedGuestImageV1 {
    pub fn entry(&self) -> &GuestImageEntryV1 {
        &self.entry
    }

    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    pub fn manifest_signer_key_id(&self) -> &str {
        &self.manifest_signer_key_id
    }

    pub fn catalog_generation(&self) -> u64 {
        self.catalog_generation
    }
}

pub fn verify_guest_image_manifest(
    bytes: &[u8],
    trust: &GuestImageManifestTrustV1,
    session: &ComputerSessionV1,
    now_unix_ms: u64,
    // Architecture detected by the trusted host probe before this call.
    expected_architecture: ComputerGuestArchitectureV1,
    controller_key_id: &str,
    controller_key: &VerifyingKey,
) -> Result<VerifiedGuestImageV1, ImageCacheErrorV1> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ImageCacheErrorV1::ManifestTooLarge);
    }
    let manifest: GuestImageManifestV1 = serde_json::from_slice(bytes)
        .map_err(|_| ImageCacheErrorV1::InvalidManifest("invalid JSON"))?;
    manifest.validate()?;
    if manifest.issued_at_unix_ms > now_unix_ms {
        return Err(ImageCacheErrorV1::InvalidManifest(
            "catalog issued in future",
        ));
    }
    if now_unix_ms > manifest.expires_at_unix_ms {
        return Err(ImageCacheErrorV1::CatalogExpired);
    }
    if manifest.catalog_generation < trust.minimum_catalog_generation {
        return Err(ImageCacheErrorV1::StaleCatalog);
    }
    session
        .validate_for_host(now_unix_ms, controller_key_id, controller_key)
        .map_err(|_| ImageCacheErrorV1::SessionAttestationInvalid)?;
    let key = trust
        .key(&manifest.signer_key_id)
        .ok_or(ImageCacheErrorV1::UntrustedManifestSigner)?;
    let signature_bytes = decode_hex(&manifest.signature_hex)?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| ImageCacheErrorV1::InvalidManifest("invalid signature"))?;
    key.verify(&manifest.signing_payload_bytes()?, &signature)
        .map_err(|_| ImageCacheErrorV1::UntrustedManifestSigner)?;
    let manifest_digest = sha256(&canonical_manifest_bytes(&manifest)?);
    if session.image_manifest_digest != manifest_digest
        || session.image_signer_key_id != manifest.signer_key_id
    {
        return Err(ImageCacheErrorV1::SessionManifestMismatch);
    }
    let entry = manifest
        .images
        .iter()
        .find(|entry| entry.image_digest == session.image_digest)
        .ok_or(ImageCacheErrorV1::ImageNotCatalogued)?;
    if entry.architecture != expected_architecture {
        return Err(ImageCacheErrorV1::ImageNotCatalogued);
    }
    if entry.status != GuestImageStatusV1::Approved {
        return Err(ImageCacheErrorV1::ImageRevoked);
    }
    if session.session_class != ComputerSessionClassV1::Disposable {
        return Err(ImageCacheErrorV1::UnsupportedSessionClass);
    }
    Ok(VerifiedGuestImageV1 {
        entry: entry.clone(),
        manifest_digest,
        manifest_signer_key_id: manifest.signer_key_id,
        catalog_version: manifest.catalog_version,
        catalog_generation: manifest.catalog_generation,
    })
}

impl GuestImageManifestV1 {
    pub fn validate(&self) -> Result<(), ImageCacheErrorV1> {
        if self.schema_version != 1
            || self.catalog_version.is_empty()
            || self.catalog_version.len() > 64
            || self.catalog_generation == 0
            || self.issued_at_unix_ms == 0
            || self.expires_at_unix_ms <= self.issued_at_unix_ms
            || self.expires_at_unix_ms - self.issued_at_unix_ms > 180 * 24 * 60 * 60 * 1000
            || !is_uuid(&self.signer_key_id)
            || !is_release_tag(&self.catalog_version)
            || self.images.is_empty()
            || self.images.len() > 16
            || self.signature_hex.len() != 128
            || !self
                .signature_hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(ImageCacheErrorV1::InvalidManifest("invalid header"));
        }
        let mut digests = BTreeSet::new();
        for image in &self.images {
            image.validate()?;
            if !digests.insert(image.image_digest.as_str()) {
                return Err(ImageCacheErrorV1::InvalidManifest(
                    "duplicate image identity",
                ));
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, ImageCacheErrorV1> {
        Ok(sha256(&canonical_manifest_bytes(self)?))
    }

    /// Bytes an offline release signer must sign for this catalog.
    pub fn signing_payload_bytes(&self) -> Result<Vec<u8>, ImageCacheErrorV1> {
        encoding::signing_payload_bytes(self)
    }
}
