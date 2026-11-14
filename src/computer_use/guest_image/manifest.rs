use std::collections::{BTreeMap, BTreeSet};

mod encoding;

#[cfg(test)]
mod tests;

use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use self::encoding::{
    canonical_manifest_bytes, is_git_revision, is_release_tag, is_sha256, is_uuid, sha256,
};
use super::ImageCacheErrorV1;

mod verification;
pub use verification::{
    VerifiedCatalogGuestImageV1, VerifiedGuestImageCatalogV1, VerifiedGuestImageV1,
    verify_guest_image_catalog, verify_guest_image_manifest,
};

pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_IMAGE_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerGuestArchitectureV1 {
    X86_64,
    Aarch64,
}

impl ComputerGuestArchitectureV1 {
    pub const fn artifact_name(self) -> &'static str {
        match self {
            Self::X86_64 => "koolade-computer-use-guest-x86_64.qcow2",
            Self::Aarch64 => "koolade-computer-use-guest-aarch64.qcow2",
        }
    }

    pub const fn debian_architecture(self) -> &'static str {
        match self {
            Self::X86_64 => "amd64",
            Self::Aarch64 => "arm64",
        }
    }
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
            || self.artifact_name != self.architecture.artifact_name()
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

    pub(super) fn key(&self, id: &str) -> Option<&VerifyingKey> {
        self.keys.get(id)
    }
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
