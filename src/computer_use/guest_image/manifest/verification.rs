use ed25519_dalek::{Signature, Verifier, VerifyingKey};

use crate::computer_use::{ComputerSessionClassV1, ComputerSessionV1};

use super::{
    ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageManifestTrustV1,
    GuestImageManifestV1, GuestImageStatusV1, MAX_MANIFEST_BYTES, encoding,
};
use crate::computer_use::guest_image::ImageCacheErrorV1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedGuestImageCatalogV1 {
    entries: Vec<GuestImageEntryV1>,
    digest: String,
    signer_key_id: String,
    catalog_version: String,
    catalog_generation: u64,
    expires_at_unix_ms: u64,
}

/// Catalog-authorized artifact suitable for pre-session download. Creating a
/// session or overlay still requires a separate `VerifiedGuestImageV1`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedCatalogGuestImageV1 {
    pub(crate) entry: GuestImageEntryV1,
    pub(crate) manifest_digest: String,
    pub(crate) manifest_signer_key_id: String,
    pub(crate) catalog_version: String,
    pub(crate) catalog_generation: u64,
    pub(crate) expires_at_unix_ms: u64,
}

impl VerifiedCatalogGuestImageV1 {
    pub fn entry(&self) -> &GuestImageEntryV1 {
        &self.entry
    }

    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }

    pub fn manifest_signer_key_id(&self) -> &str {
        &self.manifest_signer_key_id
    }

    pub fn catalog_version(&self) -> &str {
        &self.catalog_version
    }

    pub fn catalog_generation(&self) -> u64 {
        self.catalog_generation
    }

    pub fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }
}

impl VerifiedGuestImageCatalogV1 {
    pub fn approved_images(&self) -> impl Iterator<Item = &GuestImageEntryV1> {
        self.entries
            .iter()
            .filter(|entry| entry.status == GuestImageStatusV1::Approved)
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn signer_key_id(&self) -> &str {
        &self.signer_key_id
    }
    pub fn catalog_version(&self) -> &str {
        &self.catalog_version
    }
    pub fn catalog_generation(&self) -> u64 {
        self.catalog_generation
    }
    pub fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }

    pub fn approved_image(
        &self,
        architecture: ComputerGuestArchitectureV1,
    ) -> Option<&GuestImageEntryV1> {
        self.approved_images()
            .find(|entry| entry.architecture == architecture)
    }

    pub fn approved_image_for_download(
        &self,
        architecture: ComputerGuestArchitectureV1,
    ) -> Option<VerifiedCatalogGuestImageV1> {
        self.approved_image(architecture)
            .map(|entry| VerifiedCatalogGuestImageV1 {
                entry: entry.clone(),
                manifest_digest: self.digest.clone(),
                manifest_signer_key_id: self.signer_key_id.clone(),
                catalog_version: self.catalog_version.clone(),
                catalog_generation: self.catalog_generation,
                expires_at_unix_ms: self.expires_at_unix_ms,
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedGuestImageV1 {
    pub(crate) entry: GuestImageEntryV1,
    pub(crate) manifest_digest: String,
    pub(crate) manifest_signer_key_id: String,
    pub(crate) catalog_version: String,
    pub(crate) catalog_generation: u64,
    pub(crate) expires_at_unix_ms: u64,
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

    pub fn expires_at_unix_ms(&self) -> u64 {
        self.expires_at_unix_ms
    }
}

pub fn verify_guest_image_catalog(
    bytes: &[u8],
    trust: &GuestImageManifestTrustV1,
    now_unix_ms: u64,
) -> Result<VerifiedGuestImageCatalogV1, ImageCacheErrorV1> {
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
    if now_unix_ms >= manifest.expires_at_unix_ms {
        return Err(ImageCacheErrorV1::CatalogExpired);
    }
    if manifest.catalog_generation < trust.minimum_catalog_generation {
        return Err(ImageCacheErrorV1::StaleCatalog);
    }
    let key = trust
        .key(&manifest.signer_key_id)
        .ok_or(ImageCacheErrorV1::UntrustedManifestSigner)?;
    let signature_bytes = encoding::decode_hex(&manifest.signature_hex)?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| ImageCacheErrorV1::InvalidManifest("invalid signature"))?;
    key.verify(&manifest.signing_payload_bytes()?, &signature)
        .map_err(|_| ImageCacheErrorV1::UntrustedManifestSigner)?;
    Ok(VerifiedGuestImageCatalogV1 {
        digest: manifest.digest()?,
        signer_key_id: manifest.signer_key_id,
        catalog_version: manifest.catalog_version,
        catalog_generation: manifest.catalog_generation,
        expires_at_unix_ms: manifest.expires_at_unix_ms,
        entries: manifest.images,
    })
}

pub fn verify_guest_image_manifest(
    bytes: &[u8],
    trust: &GuestImageManifestTrustV1,
    session: &ComputerSessionV1,
    now_unix_ms: u64,
    expected_architecture: ComputerGuestArchitectureV1,
    controller_key_id: &str,
    controller_key: &VerifyingKey,
) -> Result<VerifiedGuestImageV1, ImageCacheErrorV1> {
    let catalog = verify_guest_image_catalog(bytes, trust, now_unix_ms)?;
    session
        .validate_for_host(now_unix_ms, controller_key_id, controller_key)
        .map_err(|_| ImageCacheErrorV1::SessionAttestationInvalid)?;
    if session.image_manifest_digest != catalog.digest
        || session.image_signer_key_id != catalog.signer_key_id
    {
        return Err(ImageCacheErrorV1::SessionManifestMismatch);
    }
    let entry = catalog
        .entries
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
        manifest_digest: catalog.digest,
        manifest_signer_key_id: catalog.signer_key_id,
        catalog_version: catalog.catalog_version,
        catalog_generation: catalog.catalog_generation,
        expires_at_unix_ms: catalog.expires_at_unix_ms,
    })
}
