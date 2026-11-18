use std::{fs, path::Path};

use anyhow::{Context, ensure};
use koolade::computer_use::guest_image::{
    ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageManifestV1, GuestImageStatusV1,
    VerifiedGuestImageCatalogV1,
};

use super::{
    GuestImageManifestTrustV1, PreviousCatalog,
    candidate::CandidateArchitecture,
    files::{read_bounded, sha256_bytes, sha256_file},
};

#[cfg(test)]
#[path = "release_assets_tests.rs"]
mod tests;

const MAX_SBOM_BYTES: usize = 16 * 1024 * 1024;
const MAX_ATTESTATION_BYTES: usize = 32 * 1024 * 1024;

pub(super) fn stage_promotion_assets(
    candidate_dir: &Path,
    attestations_path: &Path,
    previous_release_dir: &Path,
    output_dir: &Path,
    previous: &PreviousCatalog,
    candidate: &GuestImageEntryV1,
    trust: &GuestImageManifestTrustV1,
) -> anyhow::Result<()> {
    let candidate_arch = architecture_for(candidate.architecture);
    stage_one(
        candidate_dir,
        candidate_dir,
        attestations_path,
        output_dir,
        candidate,
        candidate_arch,
        true,
    )?;

    if previous.images.iter().any(|image| {
        image.status == GuestImageStatusV1::Approved && image.architecture != candidate.architecture
    }) {
        let catalog_path = previous_release_dir.join("catalog-v1.json");
        let lock_path = previous_release_dir.join("images.lock.json");
        verify_bundle_at_issued_time(&catalog_path, &lock_path, trust)?;
        for image in previous.images.iter().filter(|image| {
            image.status == GuestImageStatusV1::Approved
                && image.architecture != candidate.architecture
        }) {
            stage_one(
                previous_release_dir,
                previous_release_dir,
                &previous_release_dir.join(attestation_name(architecture_for(image.architecture))),
                output_dir,
                image,
                architecture_for(image.architecture),
                false,
            )?;
        }
    }
    Ok(())
}

fn stage_one(
    source_directory: &Path,
    image_directory: &Path,
    attestations_path: &Path,
    output_directory: &Path,
    image: &GuestImageEntryV1,
    architecture: CandidateArchitecture,
    candidate: bool,
) -> anyhow::Result<()> {
    let image_path = image_directory.join(&image.artifact_name);
    verify_file(&image_path, &image.image_digest, image.artifact_size_bytes)?;
    let sbom_path = source_directory.join(sbom_name(&image.artifact_name)?);
    verify_digest(&sbom_path, &image.sbom_sha256, MAX_SBOM_BYTES)?;
    let attestations_path = if candidate {
        attestations_path.to_path_buf()
    } else {
        legacy_or_arch_attestation(source_directory, image.architecture, attestations_path)
    };
    verify_digest(
        &attestations_path,
        &image.build_attestation_sha256,
        MAX_ATTESTATION_BYTES,
    )?;
    copy_regular_file(&image_path, &output_directory.join(&image.artifact_name))?;
    copy_regular_file(
        &sbom_path,
        &output_directory.join(sbom_name(&image.artifact_name)?),
    )?;
    copy_regular_file(
        &attestations_path,
        &output_directory.join(attestation_name(architecture)),
    )?;
    Ok(())
}

fn legacy_or_arch_attestation(
    directory: &Path,
    architecture: ComputerGuestArchitectureV1,
    proposed: &Path,
) -> std::path::PathBuf {
    if proposed.is_file() {
        return proposed.to_path_buf();
    }
    if architecture == ComputerGuestArchitectureV1::X86_64 {
        let legacy = directory.join("verified-attestations.json");
        if legacy.is_file() {
            return legacy;
        }
    }
    proposed.to_path_buf()
}

pub(super) fn verify_approved_assets(
    catalog: &VerifiedGuestImageCatalogV1,
    assets_directory: &Path,
) -> anyhow::Result<()> {
    for image in catalog.approved_images() {
        let architecture = architecture_for(image.architecture);
        ensure!(
            image.artifact_name == architecture.image_name(),
            "catalog artifact name does not match its architecture"
        );
        verify_file(
            &assets_directory.join(&image.artifact_name),
            &image.image_digest,
            image.artifact_size_bytes,
        )?;
        verify_digest(
            &assets_directory.join(sbom_name(&image.artifact_name)?),
            &image.sbom_sha256,
            MAX_SBOM_BYTES,
        )?;
        let attestations = legacy_or_arch_attestation(
            assets_directory,
            image.architecture,
            &assets_directory.join(attestation_name(architecture)),
        );
        verify_digest(
            &attestations,
            &image.build_attestation_sha256,
            MAX_ATTESTATION_BYTES,
        )?;
    }
    Ok(())
}

fn verify_bundle_at_issued_time(
    catalog_path: &Path,
    lock_path: &Path,
    trust: &GuestImageManifestTrustV1,
) -> anyhow::Result<()> {
    let catalog_bytes = read_bounded(catalog_path, 1024 * 1024)?;
    let manifest: GuestImageManifestV1 =
        serde_json::from_slice(&catalog_bytes).context("parse prior release catalog")?;
    let lock: super::GuestImageBuildLockV1 =
        serde_json::from_slice(&read_bounded(lock_path, 64 * 1024)?)
            .context("parse prior release lock")?;
    let verified = koolade::computer_use::guest_image::verify_guest_image_catalog(
        &catalog_bytes,
        trust,
        manifest.issued_at_unix_ms,
    )
    .context("verify prior release catalog signature")?;
    super::previous::validate_lock_against_catalog(&lock, &verified)
}

fn verify_file(path: &Path, expected_digest: &str, expected_size: u64) -> anyhow::Result<()> {
    ensure!(
        sha256_file(path)? == expected_digest,
        "release image digest mismatch"
    );
    ensure!(
        fs::metadata(path)?.len() == expected_size,
        "release image size mismatch"
    );
    Ok(())
}

fn verify_digest(path: &Path, expected_digest: &str, limit: usize) -> anyhow::Result<()> {
    let bytes = read_bounded(path, limit)?;
    ensure!(
        sha256_bytes(&bytes) == expected_digest,
        "release asset digest mismatch"
    );
    Ok(())
}

fn copy_regular_file(source: &Path, destination: &Path) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("stat promotion asset {}", source.display()))?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "promotion asset is not a regular file"
    );
    fs::copy(source, destination)
        .with_context(|| format!("stage promotion asset {}", source.display()))?;
    Ok(())
}

pub(super) fn attestation_name(architecture: CandidateArchitecture) -> String {
    format!("verified-attestations-{}.json", architecture.label())
}

fn sbom_name(image_name: &str) -> anyhow::Result<String> {
    Ok(format!(
        "{}.sbom.cdx.json",
        image_name
            .strip_suffix(".qcow2")
            .context("guest image filename has an unexpected extension")?
    ))
}

fn architecture_for(architecture: ComputerGuestArchitectureV1) -> CandidateArchitecture {
    match architecture {
        ComputerGuestArchitectureV1::X86_64 => CandidateArchitecture::X86_64,
        ComputerGuestArchitectureV1::Aarch64 => CandidateArchitecture::Aarch64,
    }
}
