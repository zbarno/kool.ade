mod candidate;
mod files;
mod previous;

#[cfg(test)]
#[path = "catalog/tests.rs"]
mod tests;

use std::path::Path;

use anyhow::{Context, ensure};
use ed25519_dalek::{Signer, SigningKey};
use koolade::computer_use::guest_image::{
    GuestImageManifestV1, GuestImageStatusV1, application_guest_image_manifest_trust_v1,
    verify_guest_image_catalog,
};
use serde::{Deserialize, Serialize};

use self::{
    candidate::read_candidate,
    files::{read_bounded, unix_time_ms, write_json},
    previous::{PreviousCatalog, read_previous_bundle, validate_lock_against_catalog},
};

const SIGNER_KEY_ID: &str = "76fe57c9-0102-424b-9bb2-90b7ca745006";
const CATALOG_TTL_MS: u64 = 90 * 24 * 60 * 60 * 1000;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct GuestImageBuildLockV1 {
    schema_version: u8,
    status: String,
    manifest_sha256: Option<String>,
    catalog_generation: Option<u64>,
    images: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PromotionRecordV1 {
    release_tag: String,
    source_revision: String,
    image_digest: String,
    manifest_sha256: String,
    catalog_generation: u64,
    artifact_name: String,
    artifact_size_bytes: u64,
}

pub(super) struct SigningRequest<'a> {
    pub(super) candidate_dir: &'a Path,
    pub(super) companion_binary: &'a Path,
    pub(super) output_dir: &'a Path,
    pub(super) previous_catalog_path: &'a Path,
    pub(super) previous_lock_path: &'a Path,
    pub(super) expected_source_revision: &'a str,
    pub(super) generation: u64,
    pub(super) attestations_path: &'a Path,
}

pub(super) fn sign_candidate(
    request: SigningRequest<'_>,
    signing_key: &SigningKey,
) -> anyhow::Result<()> {
    let SigningRequest {
        candidate_dir,
        companion_binary,
        output_dir,
        previous_catalog_path,
        previous_lock_path,
        expected_source_revision,
        generation,
        attestations_path,
    } = request;
    let now_ms = unix_time_ms()?;
    let candidate = read_candidate(
        candidate_dir,
        companion_binary,
        expected_source_revision,
        attestations_path,
    )?;
    let trust = application_guest_image_manifest_trust_v1()
        .map_err(|_| anyhow::anyhow!("application-pinned image trust is invalid"))?;
    let previous = read_previous_bundle(previous_catalog_path, previous_lock_path, &trust)?;
    let (manifest, lock) =
        build_signed_bundle(candidate, previous, generation, now_ms, signing_key, &trust)?;
    manifest.validate()?;

    std::fs::create_dir_all(output_dir).context("create promotion output directory")?;
    write_json(output_dir.join("catalog-v1.json"), &manifest)?;
    write_json(output_dir.join("images.lock.json"), &lock)?;
    let record = PromotionRecordV1 {
        release_tag: manifest.catalog_version.clone(),
        source_revision: manifest
            .images
            .iter()
            .find(|image| image.status == GuestImageStatusV1::Approved)
            .context("signed catalog has no approved image")?
            .source_revision
            .clone(),
        image_digest: lock
            .images
            .first()
            .context("signed catalog has no locked image")?
            .clone(),
        manifest_sha256: lock
            .manifest_sha256
            .clone()
            .context("signed catalog has no digest")?,
        catalog_generation: generation,
        artifact_name: candidate::IMAGE_NAME.to_owned(),
        artifact_size_bytes: manifest
            .images
            .iter()
            .find(|image| image.status == GuestImageStatusV1::Approved)
            .context("signed catalog has no approved image")?
            .artifact_size_bytes,
    };
    write_json(output_dir.join("promotion-record.json"), &record)?;
    Ok(())
}

fn build_signed_bundle(
    candidate: candidate::Candidate,
    previous: PreviousCatalog,
    generation: u64,
    now_ms: u64,
    signing_key: &SigningKey,
    trust: &koolade::computer_use::guest_image::GuestImageManifestTrustV1,
) -> anyhow::Result<(GuestImageManifestV1, GuestImageBuildLockV1)> {
    ensure!(
        generation > previous.generation,
        "new catalog generation must exceed the current lock generation"
    );
    let mut images = previous.images;
    for image in &mut images {
        image.status = GuestImageStatusV1::Revoked;
    }
    ensure!(
        !images
            .iter()
            .any(|image| image.image_digest == candidate.image.image_digest),
        "candidate image digest already exists in the catalog"
    );
    images.push(candidate.image.clone());
    images.sort_by(|left, right| left.image_digest.cmp(&right.image_digest));
    ensure!(images.len() <= 16, "catalog image limit exceeded");

    let mut manifest = GuestImageManifestV1 {
        schema_version: 1,
        catalog_version: files::release_tag(generation, &candidate.image.image_digest)?,
        catalog_generation: generation,
        issued_at_unix_ms: now_ms,
        expires_at_unix_ms: now_ms
            .checked_add(CATALOG_TTL_MS)
            .context("catalog expiry overflow")?,
        signer_key_id: SIGNER_KEY_ID.to_owned(),
        images,
        signature_hex: "0".repeat(128),
    };
    manifest.signature_hex = files::lower_hex(
        &signing_key
            .sign(&manifest.signing_payload_bytes()?)
            .to_bytes(),
    );
    let verified = verify_guest_image_catalog(&serde_json::to_vec(&manifest)?, trust, now_ms)
        .context("signed catalog does not verify against the application-pinned key")?;
    let lock = GuestImageBuildLockV1 {
        schema_version: 1,
        status: "promoted".to_owned(),
        manifest_sha256: Some(verified.digest().to_owned()),
        catalog_generation: Some(generation),
        images: verified
            .approved_images()
            .map(|image| image.image_digest.clone())
            .collect(),
    };
    validate_lock_against_catalog(&lock, &verified)?;
    Ok((manifest, lock))
}

pub(super) fn verify_catalog_bundle(catalog_path: &Path, lock_path: &Path) -> anyhow::Result<()> {
    let bytes = read_bounded(catalog_path, 1024 * 1024)?;
    let lock: GuestImageBuildLockV1 = serde_json::from_slice(&read_bounded(lock_path, 64 * 1024)?)
        .context("parse guest image lock")?;
    let trust = application_guest_image_manifest_trust_v1()
        .map_err(|_| anyhow::anyhow!("application-pinned image trust is invalid"))?;
    let catalog = verify_guest_image_catalog(&bytes, &trust, unix_time_ms()?)
        .context("verify signed guest image catalog")?;
    validate_lock_against_catalog(&lock, &catalog)?;
    ensure!(
        catalog.approved_images().count() == 1,
        "promoted catalog must contain exactly one approved image"
    );
    Ok(())
}

pub(super) fn verify_release_image(
    catalog_path: &Path,
    lock_path: &Path,
    image_path: &Path,
) -> anyhow::Result<()> {
    verify_catalog_bundle(catalog_path, lock_path)?;
    let bytes = read_bounded(catalog_path, 1024 * 1024)?;
    let trust = application_guest_image_manifest_trust_v1()
        .map_err(|_| anyhow::anyhow!("application-pinned image trust is invalid"))?;
    let catalog = verify_guest_image_catalog(&bytes, &trust, unix_time_ms()?)?;
    let entry = catalog
        .approved_images()
        .next()
        .context("promoted image is missing")?;
    ensure!(
        files::sha256_file(image_path)? == entry.image_digest,
        "release asset digest mismatch"
    );
    ensure!(
        std::fs::metadata(image_path)?.len() == entry.artifact_size_bytes,
        "release asset size mismatch"
    );
    Ok(())
}
