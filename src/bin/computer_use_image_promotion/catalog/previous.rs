use std::path::Path;

use anyhow::{Context, ensure};
use koolade::computer_use::guest_image::{
    GuestImageEntryV1, GuestImageManifestTrustV1, GuestImageManifestV1,
    VerifiedGuestImageCatalogV1, verify_guest_image_catalog,
};
use serde_json::Value;

use super::{GuestImageBuildLockV1, files::read_bounded};

pub(super) struct PreviousCatalog {
    pub(super) generation: u64,
    pub(super) images: Vec<GuestImageEntryV1>,
}

pub(super) fn read_previous_bundle(
    catalog_path: &Path,
    lock_path: &Path,
    trust: &GuestImageManifestTrustV1,
) -> anyhow::Result<PreviousCatalog> {
    let catalog_bytes = read_bounded(catalog_path, 1024 * 1024)?;
    let lock: GuestImageBuildLockV1 = serde_json::from_slice(&read_bounded(lock_path, 64 * 1024)?)
        .context("parse previous image lock")?;
    ensure!(
        lock.schema_version == 1,
        "unsupported previous image lock schema"
    );
    if lock.status == "not_promoted" {
        ensure!(
            lock.manifest_sha256.is_none(),
            "not-promoted lock has a manifest digest"
        );
        ensure!(
            lock.catalog_generation.is_none(),
            "not-promoted lock has a generation"
        );
        ensure!(lock.images.is_empty(), "not-promoted lock lists images");
        ensure!(
            serde_json::from_slice::<Value>(&catalog_bytes)?
                == serde_json::json!({"status":"not_promoted"}),
            "not-promoted lock must use the catalog sentinel"
        );
        return Ok(PreviousCatalog {
            generation: 0,
            images: Vec::new(),
        });
    }
    ensure!(
        lock.status == "promoted",
        "unknown previous image lock status"
    );
    let previous: GuestImageManifestV1 =
        serde_json::from_slice(&catalog_bytes).context("parse previous signed catalog")?;
    ensure!(
        lock.catalog_generation == Some(previous.catalog_generation),
        "previous catalog generation disagrees with lock"
    );
    let verified = verify_guest_image_catalog(&catalog_bytes, trust, previous.issued_at_unix_ms)
        .context("previous catalog signature is not trusted")?;
    ensure!(
        lock.manifest_sha256.as_deref() == Some(verified.digest()),
        "previous catalog digest disagrees with lock"
    );
    validate_lock_against_catalog(&lock, &verified)?;
    Ok(PreviousCatalog {
        generation: verified.catalog_generation(),
        images: previous.images,
    })
}

pub(super) fn validate_lock_against_catalog(
    lock: &GuestImageBuildLockV1,
    catalog: &VerifiedGuestImageCatalogV1,
) -> anyhow::Result<()> {
    ensure!(
        lock.schema_version == 1,
        "unsupported guest image lock schema"
    );
    ensure!(
        lock.status == "promoted",
        "guest image lock is not promoted"
    );
    ensure!(
        lock.manifest_sha256.as_deref() == Some(catalog.digest()),
        "guest image lock manifest digest mismatch"
    );
    ensure!(
        lock.catalog_generation == Some(catalog.catalog_generation()),
        "guest image lock generation mismatch"
    );
    let mut locked_images = lock.images.clone();
    let mut approved_images: Vec<_> = catalog
        .approved_images()
        .map(|image| image.image_digest.clone())
        .collect();
    locked_images.sort();
    approved_images.sort();
    ensure!(
        locked_images == approved_images,
        "guest image lock image list mismatch"
    );
    Ok(())
}
