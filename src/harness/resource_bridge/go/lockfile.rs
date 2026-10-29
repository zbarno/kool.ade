use crate::harness::DependencyPackageIdentity;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Read, path::Path};

mod sums;
use sums::parse_sums;

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_SUM_BYTES: u64 = 8 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 4096;
const MAX_SUM_ENTRIES: usize = 10_000;
const MAX_REQUIRED_MODULES: usize = 2_000;
const PROXY_SOURCE: &str = "https://proxy.golang.org";

#[cfg(test)]
#[path = "lockfile/tests.rs"]
mod tests;

#[derive(Debug)]
pub(super) struct GoLock {
    pub(super) packages: Vec<DependencyPackageIdentity>,
    pub(super) manifest_inputs: Vec<(String, serde_json::Value)>,
}

pub(super) fn collect(root: &Path) -> anyhow::Result<GoLock> {
    let (manifest, sums) = read_inputs(root)?;
    let inputs = manifest_inputs_from_bytes(&manifest, &sums)?;
    let packages = package_identities_from_contents(std::str::from_utf8(&sums)?)?;
    Ok(GoLock {
        packages,
        manifest_inputs: inputs,
    })
}

pub(super) fn identity(lock: &GoLock) -> anyhow::Result<String> {
    Ok(format!(
        "sha256:{:x}",
        Sha256::digest(serde_json::to_vec(&(
            &lock.packages,
            &lock.manifest_inputs
        ))?)
    ))
}

pub(super) fn package_identities(root: &Path) -> anyhow::Result<Vec<DependencyPackageIdentity>> {
    let (_, sums) = read_inputs(root)?;
    package_identities_from_contents(std::str::from_utf8(&sums)?)
}

pub(super) fn package_identities_from_contents(
    contents: &str,
) -> anyhow::Result<Vec<DependencyPackageIdentity>> {
    Ok(parse_sums(contents)?
        .into_iter()
        .map(|entry| DependencyPackageIdentity {
            package: entry.module,
            version: entry.version,
            source: PROXY_SOURCE.into(),
            integrity: entry.hash,
        })
        .collect())
}

pub(super) fn manifest_inputs(root: &Path) -> anyhow::Result<Vec<(String, serde_json::Value)>> {
    let (manifest, sums) = read_inputs(root)?;
    manifest_inputs_from_bytes(&manifest, &sums)
}

pub(super) fn manifest_inputs_from_bytes(
    manifest: &[u8],
    sums: &[u8],
) -> anyhow::Result<Vec<(String, serde_json::Value)>> {
    let manifest_text = std::str::from_utf8(manifest)?;
    let sum_text = std::str::from_utf8(sums)?;
    let requirements = parse_manifest(manifest_text)?;
    let entries = parse_sums(sum_text)?;
    for (module, version) in &requirements {
        let escaped = escape_module_path(module)?;
        anyhow::ensure!(
            entries
                .iter()
                .any(|entry| entry.module == escaped && entry.version == *version)
                && entries.iter().any(|entry| {
                    entry.module == escaped && entry.version == format!("{version}/go.mod")
                }),
            "go.sum must pin both the module archive and go.mod for every required module"
        );
    }
    let mut inputs = Vec::with_capacity(2);
    for (path, bytes) in [("go.mod", manifest), ("go.sum", sums)] {
        inputs.push((
            path.into(),
            serde_json::json!({ "sha256": format!("{:x}", Sha256::digest(bytes)) }),
        ));
    }
    Ok(inputs)
}

fn read_inputs(root: &Path) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    for workspace in ["go.work", "go.work.sum"] {
        anyhow::ensure!(
            !root.join(workspace).try_exists()?,
            "Go workspace files are unsupported for brokered restores"
        );
    }
    let manifest = read_regular_file(&root.join("go.mod"), MAX_MANIFEST_BYTES, "go.mod")?;
    let sums = read_regular_file(&root.join("go.sum"), MAX_SUM_BYTES, "go.sum")?;
    Ok((manifest, sums))
}

fn read_regular_file(path: &Path, limit: u64, name: &str) -> anyhow::Result<Vec<u8>> {
    let file = super::super::secure_file::open_read_nofollow(path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "{name} must be a regular file"
    );
    anyhow::ensure!(metadata.len() <= limit, "{name} exceeds its scan limit");
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= limit,
        "{name} grew beyond its scan limit"
    );
    Ok(bytes)
}

fn parse_manifest(contents: &str) -> anyhow::Result<Vec<(String, String)>> {
    anyhow::ensure!(
        contents.len() as u64 <= MAX_MANIFEST_BYTES,
        "go.mod exceeds its scan limit"
    );
    let mut module = None;
    let mut go_version = None;
    let mut requirements = BTreeMap::<String, String>::new();
    let mut in_require_block = false;
    for (line_number, original) in contents.lines().enumerate() {
        anyhow::ensure!(
            original.len() <= MAX_LINE_BYTES,
            "go.mod line exceeds its scan limit"
        );
        let line = original
            .split_once("//")
            .map_or(original, |(code, _)| code)
            .trim();
        if line.is_empty() {
            continue;
        }
        if in_require_block {
            if line == ")" {
                in_require_block = false;
                continue;
            }
            let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
            anyhow::ensure!(
                fields.len() == 2,
                "go.mod require entry on line {} is not an exact module/version pair",
                line_number + 1
            );
            insert_requirement(&mut requirements, fields[0], fields[1])?;
            continue;
        }
        if let Some(value) = line.strip_prefix("module ") {
            anyhow::ensure!(
                module.is_none(),
                "go.mod declares more than one module path"
            );
            validate_module_path(value)?;
            module = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("go ") {
            anyhow::ensure!(
                go_version.is_none(),
                "go.mod declares more than one Go version"
            );
            validate_go_version(value)?;
            go_version = Some(value.to_owned());
        } else if line == "require (" {
            anyhow::ensure!(!in_require_block, "go.mod contains a nested require block");
            in_require_block = true;
        } else if let Some(value) = line.strip_prefix("require ") {
            let fields = value.split_ascii_whitespace().collect::<Vec<_>>();
            anyhow::ensure!(
                fields.len() == 2,
                "go.mod require directive is not an exact module/version pair"
            );
            insert_requirement(&mut requirements, fields[0], fields[1])?;
        } else {
            anyhow::bail!(
                "go.mod contains an unsupported directive; replacements, workspaces, toolchains, and exclusions are not supported"
            );
        }
        anyhow::ensure!(
            requirements.len() <= MAX_REQUIRED_MODULES,
            "go.mod has too many requirements"
        );
    }
    anyhow::ensure!(!in_require_block, "go.mod require block is not closed");
    anyhow::ensure!(module.is_some(), "go.mod module path is missing");
    anyhow::ensure!(go_version.is_some(), "go.mod Go version is missing");
    anyhow::ensure!(
        !requirements.is_empty(),
        "go.mod has no external module requirements"
    );
    Ok(requirements.into_iter().collect())
}

fn insert_requirement(
    requirements: &mut BTreeMap<String, String>,
    module: &str,
    version: &str,
) -> anyhow::Result<()> {
    validate_module_path(module)?;
    validate_version(version)?;
    anyhow::ensure!(
        requirements.insert(module.into(), version.into()).is_none(),
        "go.mod contains a duplicate module requirement"
    );
    Ok(())
}

fn validate_module_path(value: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        value.len() <= 512
            && value.is_ascii()
            && value.split('/').count() >= 2
            && value.split('/').all(|part| {
                !part.is_empty()
                    && !part.starts_with('.')
                    && !part.ends_with('.')
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b".!-_~+".contains(&byte))
            })
            && value
                .split('/')
                .next()
                .is_some_and(|domain| domain.contains('.')),
        "Go module path is outside the supported public module path subset"
    );
    Ok(())
}

fn escape_module_path(value: &str) -> anyhow::Result<String> {
    validate_module_path(value)?;
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_uppercase() {
            escaped.push('!');
            escaped.push((byte + 32) as char);
        } else {
            escaped.push(byte as char);
        }
    }
    Ok(escaped)
}

fn validate_version(value: &str) -> anyhow::Result<()> {
    let version = value.strip_suffix("+incompatible").unwrap_or(value);
    let (release, prerelease) = version.split_once('-').unwrap_or((version, ""));
    let components = release
        .strip_prefix('v')
        .unwrap_or_default()
        .split('.')
        .collect::<Vec<_>>();
    let supported_build = value
        .strip_suffix("+incompatible")
        .is_some_and(|base| !base.contains('+'));
    anyhow::ensure!(
        value.len() <= 256 && value.starts_with('v') && (!value.contains('+') || supported_build),
        "Go module version must be an exact semantic version"
    );
    anyhow::ensure!(
        components.len() == 3
            && components.iter().all(|component| {
                !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
            })
            && prerelease
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-".contains(&byte)),
        "Go module version must be an exact semantic version"
    );
    Ok(())
}

fn validate_go_version(value: &str) -> anyhow::Result<()> {
    let fields = value.split('.').collect::<Vec<_>>();
    anyhow::ensure!(
        (2..=3).contains(&fields.len())
            && fields.iter().all(|field| {
                !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit())
            }),
        "go.mod Go version is malformed"
    );
    Ok(())
}
