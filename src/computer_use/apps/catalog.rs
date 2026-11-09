use std::collections::{BTreeMap, BTreeSet};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

mod validation;
use self::validation::{
    bounded_label, decode_hex, sha256, valid_package_name, valid_uuid, valid_version,
};
pub(super) use self::validation::{valid_id, valid_sha256};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestAppArchitectureV1 {
    X86_64,
    Aarch64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestAppCapabilityV1 {
    GraphicalDisplay,
    GuestFilesystem,
    GuestNetwork,
    Microphone,
    Camera,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestAppProvenanceV1 {
    AppOwnedOfflinePackage,
    SignedDistributionRepository,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestAppInstallRecipeV1 {
    VerifiedDebPackageV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestAppRemovalV1 {
    RemoveExactPackageV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestAppRollbackV1 {
    RestoreSessionOverlayV1,
}

/// Provenance identity for an immutable package; it is not a download URL or command.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestAppPackageSourceV1 {
    pub source_id: String,
    pub package_id: String,
    pub publisher: String,
    pub provenance: GuestAppProvenanceV1,
    pub version: String,
    pub package_sha256: String,
    #[serde(default)]
    pub rollback_version: Option<String>,
    #[serde(default)]
    pub rollback_package_sha256: Option<String>,
}

/// A release-reviewed application identity. Executable IDs resolve only in the
/// guest's fixed executable registry; they never contain paths or arguments.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestAppManifestV1 {
    pub schema_version: u16,
    pub app_id: String,
    pub display_name: String,
    pub package: GuestAppPackageSourceV1,
    pub supported_image_digests: BTreeSet<String>,
    pub supported_architectures: BTreeSet<GuestAppArchitectureV1>,
    pub executable_id: String,
    pub required_capabilities: BTreeSet<GuestAppCapabilityV1>,
    pub install_recipe: GuestAppInstallRecipeV1,
    pub removal: GuestAppRemovalV1,
    pub rollback: GuestAppRollbackV1,
    pub integrity_evidence_sha256: String,
}

impl GuestAppManifestV1 {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema_version != 1
            || !valid_id(&self.app_id)
            || !bounded_label(&self.display_name, 96)
            || !valid_id(&self.package.source_id)
            || !valid_package_name(&self.package.package_id)
            || !bounded_label(&self.package.publisher, 96)
            || !valid_version(&self.package.version)
            || !valid_sha256(&self.package.package_sha256)
            || self
                .package
                .rollback_version
                .as_deref()
                .is_some_and(|version| !valid_version(version))
            || self
                .package
                .rollback_package_sha256
                .as_deref()
                .is_some_and(|digest| !valid_sha256(digest))
            || self.package.rollback_version.is_some()
                != self.package.rollback_package_sha256.is_some()
            || self.supported_image_digests.is_empty()
            || self.supported_image_digests.len() > 16
            || self
                .supported_image_digests
                .iter()
                .any(|value| !valid_sha256(value))
            || self.supported_architectures.is_empty()
            || self.supported_architectures.len() > 2
            || !valid_id(&self.executable_id)
            || self.required_capabilities.is_empty()
            || self.required_capabilities.len() > 8
            || !valid_sha256(&self.integrity_evidence_sha256)
        {
            return Err("guest app manifest is invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestAppStatusV1 {
    Preinstalled,
    TrustedCatalog,
    OperatorApprovedAdditional,
    Unsupported,
}

/// Canonical release catalog signed by an app-pinned release key.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GuestApplicationCatalogV1 {
    pub schema_version: u16,
    pub catalog_id: String,
    pub generation: u64,
    pub image_digest: String,
    pub architecture: GuestAppArchitectureV1,
    pub signer_key_id: String,
    pub apps: Vec<GuestAppManifestV1>,
    pub signature_hex: String,
}

#[derive(Clone, Debug, Default)]
pub struct GuestApplicationCatalogTrustV1 {
    keys: BTreeMap<String, VerifyingKey>,
}

impl GuestApplicationCatalogTrustV1 {
    /// Pin the application catalog to a key shipped by this app release.
    /// Catalog signatures use their own domain prefix even where deployments
    /// currently reuse the guest-image release key.
    pub fn application_pinned() -> Result<Self, &'static str> {
        let (key_id, key) = crate::computer_use::guest_image::application_catalog_signer()
            .map_err(|_| "guest app catalog trust is unavailable")?;
        Self::from_application_pinned_keys([(key_id, key)])
    }

    pub fn from_application_pinned_keys(
        keys: impl IntoIterator<Item = (String, VerifyingKey)>,
    ) -> Result<Self, &'static str> {
        let mut trusted = BTreeMap::new();
        for (id, key) in keys {
            if !valid_uuid(&id) || trusted.insert(id, key).is_some() {
                return Err("guest app catalog trust keys are invalid");
            }
        }
        if trusted.is_empty() {
            return Err("guest app catalog requires a pinned signing key");
        }
        Ok(Self { keys: trusted })
    }
}

#[derive(Clone, Debug)]
pub struct VerifiedGuestApplicationCatalogV1 {
    catalog: GuestApplicationCatalogV1,
    digest: String,
}

impl VerifiedGuestApplicationCatalogV1 {
    pub fn verify(
        catalog: GuestApplicationCatalogV1,
        trust: &GuestApplicationCatalogTrustV1,
    ) -> Result<Self, &'static str> {
        if catalog.schema_version != 1
            || !valid_uuid(&catalog.catalog_id)
            || catalog.generation == 0
            || !valid_sha256(&catalog.image_digest)
            || !valid_uuid(&catalog.signer_key_id)
            || catalog.apps.is_empty()
            || catalog.apps.len() > 64
            || catalog.signature_hex.len() != 128
            || !catalog
                .signature_hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("guest app catalog header is invalid");
        }
        let key = trust
            .keys
            .get(&catalog.signer_key_id)
            .ok_or("guest app catalog signer is not trusted")?;
        let signature_bytes = decode_hex(&catalog.signature_hex)?;
        let signature = Signature::from_slice(&signature_bytes)
            .map_err(|_| "guest app catalog signature is malformed")?;
        key.verify(&catalog.signing_bytes()?, &signature)
            .map_err(|_| "guest app catalog signature is invalid")?;
        let mut ids = BTreeSet::new();
        for app in &catalog.apps {
            app.validate()?;
            if !ids.insert(&app.app_id)
                || !app.supported_image_digests.contains(&catalog.image_digest)
                || !app.supported_architectures.contains(&catalog.architecture)
            {
                return Err("guest app catalog contains an incompatible or duplicate app");
            }
        }
        let digest = sha256(&catalog.signing_bytes()?);
        Ok(Self { catalog, digest })
    }

    pub fn catalog(&self) -> &GuestApplicationCatalogV1 {
        &self.catalog
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn app(&self, app_id: &str) -> Option<&GuestAppManifestV1> {
        self.catalog.apps.iter().find(|app| app.app_id == app_id)
    }
}

impl GuestApplicationCatalogV1 {
    pub fn signing_bytes(&self) -> Result<Vec<u8>, &'static str> {
        let mut unsigned = self.clone();
        unsigned.signature_hex.clear();
        let encoded =
            serde_json::to_vec(&unsigned).map_err(|_| "guest app catalog encoding failed")?;
        let mut bytes = b"koolade.guest-app-catalog.v1\0".to_vec();
        bytes.extend(encoded);
        Ok(bytes)
    }
}
