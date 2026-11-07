use std::path::Path;

use anyhow::{Context, ensure};
use koolade::computer_use::guest_image::{
    ComputerGuestArchitectureV1, GuestImageEntryV1, GuestImageStatusV1,
};
use serde_json::Value;

use super::files::{read_bounded, sha256_bytes, sha256_file};

pub(super) const IMAGE_BASENAME: &str = "koolade-computer-use-guest-x86_64";
pub(super) const IMAGE_NAME: &str = "koolade-computer-use-guest-x86_64.qcow2";
const MAX_ATTESTATION_BYTES: usize = 32 * 1024 * 1024;

pub(super) struct Candidate {
    pub(super) image: GuestImageEntryV1,
}

pub(super) fn read_candidate(
    directory: &Path,
    companion_binary: &Path,
    expected_source_revision: &str,
    attestations_path: &Path,
) -> anyhow::Result<Candidate> {
    ensure!(
        expected_source_revision.len() == 40
            && expected_source_revision
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "expected source revision is not a canonical Git SHA"
    );
    let image_path = directory.join(format!("{IMAGE_BASENAME}.qcow2"));
    let image_digest = verify_sha256sum_sidecar(
        directory.join(format!("{IMAGE_BASENAME}.sha256")),
        &image_path,
        IMAGE_NAME,
    )?;
    let image_size_bytes =
        read_virtual_size(&directory.join(format!("{IMAGE_BASENAME}.qemu-info.json")))?;
    let artifact_size_bytes = std::fs::metadata(&image_path)
        .context("stat candidate qcow2")?
        .len();
    ensure!(artifact_size_bytes > 0, "candidate artifact is empty");

    let agent_name = format!("{IMAGE_BASENAME}.agent");
    let companion_sha256 = read_sha256sum_sidecar(
        directory.join(format!("{IMAGE_BASENAME}.agent.sha256")),
        &agent_name,
    )?;
    ensure!(
        sha256_file(companion_binary)? == companion_sha256,
        "candidate companion digest does not match the source-built binary"
    );
    let recipe_components = directory.join(format!("{IMAGE_BASENAME}.recipe-components.sha256"));
    let build_recipe_sha256 = sha256_file(&recipe_components)?;
    let declared_recipe =
        std::fs::read_to_string(directory.join(format!("{IMAGE_BASENAME}.recipe.sha256")))
            .context("read recipe digest")?;
    ensure!(
        declared_recipe.trim() == build_recipe_sha256,
        "recipe digest sidecar mismatch"
    );

    let sbom_path = directory.join(format!("{IMAGE_BASENAME}.sbom.cdx.json"));
    let sbom_bytes = read_bounded(&sbom_path, 16 * 1024 * 1024)?;
    let sbom: Value = serde_json::from_slice(&sbom_bytes).context("parse CycloneDX SBOM")?;
    ensure!(
        sbom.get("bomFormat").and_then(Value::as_str) == Some("CycloneDX"),
        "SBOM is not CycloneDX"
    );
    let sbom_sha256 = verify_sha256sum_sidecar(
        directory.join(format!("{IMAGE_BASENAME}.sbom.sha256")),
        &sbom_path,
        &format!("{IMAGE_BASENAME}.sbom.cdx.json"),
    )?;
    ensure!(
        sbom_sha256 == sha256_bytes(&sbom_bytes),
        "SBOM digest mismatch"
    );

    let source_revision =
        std::fs::read_to_string(directory.join(format!("{IMAGE_BASENAME}.source-revision")))
            .context("read candidate source revision")?
            .trim()
            .to_owned();
    ensure!(
        source_revision == expected_source_revision,
        "candidate source revision does not match workflow run"
    );
    let attestations = read_bounded(attestations_path, MAX_ATTESTATION_BYTES)?;
    let parsed: Value = serde_json::from_slice(&attestations)
        .context("parse verified provenance and SBOM attestations")?;
    ensure!(
        parsed.as_array().is_some_and(|items| items.len() >= 2),
        "promotion requires verified provenance and SBOM attestations"
    );

    Ok(Candidate {
        image: GuestImageEntryV1 {
            architecture: ComputerGuestArchitectureV1::X86_64,
            image_digest,
            image_size_bytes,
            artifact_size_bytes,
            artifact_name: IMAGE_NAME.to_owned(),
            guest_release: format!("debian-bookworm-{}", &source_revision[..12]),
            source_revision: source_revision.clone(),
            build_recipe_sha256,
            sbom_sha256,
            build_attestation_sha256: sha256_bytes(&attestations),
            companion_protocol_version:
                koolade::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
            companion_sha256,
            status: GuestImageStatusV1::Approved,
        },
    })
}

fn read_virtual_size(path: &Path) -> anyhow::Result<u64> {
    let value: Value = serde_json::from_slice(&read_bounded(path, 64 * 1024)?)
        .context("parse normalized qemu-img metadata")?;
    let size = value
        .get("virtual-size")
        .and_then(Value::as_u64)
        .context("qemu-img metadata has no virtual-size")?;
    ensure!(
        (2 * 1024 * 1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&size),
        "virtual disk size is outside catalog bounds"
    );
    Ok(size)
}

fn verify_sha256sum_sidecar(
    sidecar: impl AsRef<Path>,
    artifact: &Path,
    expected_name: &str,
) -> anyhow::Result<String> {
    let declared = read_sha256sum_sidecar(sidecar, expected_name)?;
    let actual = sha256_file(artifact)?;
    ensure!(
        declared == actual,
        "digest sidecar mismatch for {}",
        artifact.display()
    );
    Ok(actual)
}

fn read_sha256sum_sidecar(
    sidecar: impl AsRef<Path>,
    expected_name: &str,
) -> anyhow::Result<String> {
    let path = sidecar.as_ref();
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("read digest sidecar {}", path.display()))?;
    let mut fields = content.split_whitespace();
    let digest = fields.next().context("digest sidecar is empty")?;
    let file_name = fields.next().context("digest sidecar has no filename")?;
    ensure!(fields.next().is_none(), "digest sidecar has extra fields");
    ensure!(
        std::path::Path::new(file_name)
            .file_name()
            .and_then(|name| name.to_str())
            == Some(expected_name),
        "digest sidecar names a different file"
    );
    ensure!(
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "digest sidecar is not canonical SHA-256"
    );
    Ok(format!("sha256:{digest}"))
}
