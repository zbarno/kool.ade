use super::AuthorizedImageExtensionRequest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Cursor;

const MAX_PACKAGE_COUNT: usize = 256;
const MAX_PACKAGE_BYTES: usize = 512 * 1024 * 1024;
const DOCKERFILE: &str = "FROM __BASE__\nLABEL io.koolade.content=__CONTENT__ io.koolade.base-digest=__BASE_DIGEST__\nCOPY packages/ /opt/koolade-packages/\nRUN --network=none dpkg --force-confold -i /opt/koolade-packages/*.deb && rm -rf /opt/koolade-packages\n";

/// Exact repository identity bound to the bytes admitted by the trusted broker.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PackageArtifactIdentity {
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub source: String,
    pub sha256: String,
}

/// Immutable verified bytes. This type is only created after the broker checks
/// the signed index entry and independently hashes the downloaded artifact.
#[derive(Clone, Debug)]
pub(crate) struct VerifiedPackageArtifact {
    pub identity: PackageArtifactIdentity,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(crate) struct GeneratedImageContext {
    pub content_identity: String,
    pub base_digest: String,
    pub package_manifest: Vec<PackageArtifactIdentity>,
    pub tar: Vec<u8>,
}

/// `dpkg-query` output from the just-built image must match the authorized
/// package names, exact versions, and architectures before publishing its digest.
pub(crate) fn verify_installed_manifest(
    output: &str,
    expected: &[PackageArtifactIdentity],
) -> anyhow::Result<()> {
    let installed = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<std::collections::BTreeSet<_>>();
    anyhow::ensure!(
        installed.len() == expected.len()
            && expected.iter().all(|package| {
                installed.contains(
                    format!(
                        "{}={}={}",
                        package.name, package.version, package.architecture
                    )
                    .as_str(),
                )
            }),
        "derived image package manifest differs from the authorized closure"
    );
    Ok(())
}

pub(crate) fn generate_context(
    request: &AuthorizedImageExtensionRequest,
    mut packages: Vec<VerifiedPackageArtifact>,
) -> anyhow::Result<GeneratedImageContext> {
    anyhow::ensure!(
        !packages.is_empty() && packages.len() <= MAX_PACKAGE_COUNT,
        "verified package closure is empty or exceeds its limit"
    );
    packages.sort_by(|left, right| left.identity.name.cmp(&right.identity.name));
    anyhow::ensure!(
        packages
            .windows(2)
            .all(|pair| pair[0].identity.name < pair[1].identity.name),
        "verified package closure contains duplicate package names"
    );
    anyhow::ensure!(
        packages
            .iter()
            .map(|artifact| &artifact.identity)
            .eq(request.package_manifest.iter()),
        "verified package bytes differ from the authorized immutable closure"
    );
    let mut total_bytes = 0_usize;
    for artifact in &packages {
        validate_identity(&artifact.identity, &request.target_platform)?;
        total_bytes = total_bytes
            .checked_add(artifact.bytes.len())
            .ok_or_else(|| anyhow::anyhow!("package closure size overflow"))?;
        anyhow::ensure!(
            total_bytes <= MAX_PACKAGE_BYTES
                && format!("{:x}", Sha256::digest(&artifact.bytes)) == artifact.identity.sha256,
            "verified package artifact has an invalid size or SHA256"
        );
    }
    let manifest = packages
        .iter()
        .map(|artifact| artifact.identity.clone())
        .collect::<Vec<_>>();
    let identity = serde_json::to_vec(&(
        "koolade-debian-image-v1",
        &request.base_digest,
        &request.target_platform,
        &manifest,
    ))?;
    let content_identity = format!("sha256:{:x}", Sha256::digest(identity));
    let dockerfile = DOCKERFILE
        .replace("__BASE__", &request.base_reference)
        .replace("__CONTENT__", &content_identity)
        .replace("__BASE_DIGEST__", &request.base_digest);
    let mut tar = tar::Builder::new(Cursor::new(Vec::new()));
    append_file(&mut tar, "Dockerfile", dockerfile.as_bytes())?;
    for (index, artifact) in packages.iter().enumerate() {
        append_file(
            &mut tar,
            &format!("packages/{index:04}.deb"),
            &artifact.bytes,
        )?;
    }
    let tar = tar.into_inner()?.into_inner();
    Ok(GeneratedImageContext {
        content_identity,
        base_digest: request.base_digest.clone(),
        package_manifest: manifest,
        tar,
    })
}

fn append_file(
    tar: &mut tar::Builder<Cursor<Vec<u8>>>,
    path: &str,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o444);
    header.set_cksum();
    tar.append_data(&mut header, path, bytes)?;
    Ok(())
}

pub(crate) fn validate_identity(
    identity: &PackageArtifactIdentity,
    platform: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !identity.name.is_empty()
            && identity.name.len() <= 128
            && identity
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"+.-".contains(&byte))
            && !identity.version.is_empty()
            && identity.version.len() <= 128
            && identity
                .version
                .bytes()
                .all(|byte| { byte.is_ascii_alphanumeric() || b".+:~_-".contains(&byte) })
            && (identity.architecture == "all"
                || platform.strip_prefix("linux/") == Some(identity.architecture.as_str()))
            && valid_repository_source(&identity.source)
            && identity.sha256.len() == 64
            && identity.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "verified package identity does not match an approved Debian artifact"
    );
    Ok(())
}

fn valid_repository_source(source: &str) -> bool {
    let Some(path) = source.strip_prefix("https://deb.debian.org/debian/pool/") else {
        return false;
    };
    path.len() <= 2_048
        && !path.is_empty()
        && !path.starts_with('/')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && !source.contains(['?', '#', '@', '\\'])
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod tests;
