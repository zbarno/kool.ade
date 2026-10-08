use crate::harness::DependencyPackageIdentity;
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

const MAX_LOCKFILES: usize = 128;
const MAX_LOCKFILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TREE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_PACKAGES: usize = 2_000;
const MAX_DEPTH: usize = 20;

pub(in crate::harness::resource_bridge::npm) fn packages(
    root: &Path,
) -> anyhow::Result<Vec<DependencyPackageIdentity>> {
    let root = root.canonicalize()?;
    let mut total = 0_u64;
    let mut packages = BTreeSet::new();
    for path in lockfiles(&root)? {
        let metadata = fs::metadata(&path)?;
        anyhow::ensure!(
            metadata.len() <= MAX_LOCKFILE_BYTES,
            "npm lockfile is larger than 32 MiB"
        );
        total = total.saturating_add(metadata.len());
        anyhow::ensure!(
            total <= MAX_TREE_BYTES,
            "npm lockfiles exceed the scan budget"
        );
        packages.extend(packages_from_bytes(&fs::read(path)?)?);
        anyhow::ensure!(
            packages.len() <= MAX_PACKAGES,
            "npm lockfile package limit exceeded"
        );
    }
    Ok(packages.into_iter().collect())
}

pub(in crate::harness::resource_bridge::npm) fn packages_from_bytes(
    bytes: &[u8],
) -> anyhow::Result<Vec<DependencyPackageIdentity>> {
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_LOCKFILE_BYTES,
        "npm lockfile is larger than 32 MiB"
    );
    let value: Value = serde_json::from_slice(bytes)?;
    let mut packages = BTreeSet::new();
    visit(&value, None, &mut packages)?;
    anyhow::ensure!(
        packages.len() <= MAX_PACKAGES,
        "npm lockfile package limit exceeded"
    );
    Ok(packages.into_iter().collect())
}

fn lockfiles(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut paths = Vec::new();
    while let Some((directory, depth)) = pending.pop() {
        anyhow::ensure!(
            depth <= MAX_DEPTH,
            "project tree exceeds the npm lockfile scan depth"
        );
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !matches!(
                    entry.file_name().to_str(),
                    Some(
                        ".git"
                            | "node_modules"
                            | ".koolade-packet"
                            | "target"
                            | "dist"
                            | "build"
                            | "coverage"
                            | "vendor"
                            | "obj"
                            | "bin"
                    )
                ) {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            if matches!(
                path.file_name().and_then(|name| name.to_str()),
                Some("package-lock.json" | "npm-shrinkwrap.json")
            ) {
                anyhow::ensure!(
                    paths.len() < MAX_LOCKFILES,
                    "project contains too many npm lockfiles"
                );
                paths.push(path);
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn visit(
    value: &Value,
    name_hint: Option<&str>,
    packages: &mut BTreeSet<DependencyPackageIdentity>,
) -> anyhow::Result<()> {
    let Some(object) = value.as_object() else {
        if let Some(values) = value.as_array() {
            for child in values {
                visit(child, name_hint, packages)?;
            }
        }
        return Ok(());
    };

    let is_link = object.get("link").and_then(Value::as_bool) == Some(true);
    if is_link && let Some(resolved) = object.get("resolved").and_then(Value::as_str) {
        anyhow::ensure!(
            !super::is_remote_http_url(resolved),
            "npm lockfile link entries cannot reference remote URLs"
        );
    }
    if let Some(resolved) = object.get("resolved").and_then(Value::as_str)
        && !is_link
        && !resolved.starts_with("file:")
        && !resolved.starts_with("link:")
    {
        let package = object
            .get("name")
            .and_then(Value::as_str)
            .or(name_hint)
            .ok_or_else(|| anyhow::anyhow!("Locked npm package name is missing"))?;
        let version = object
            .get("version")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Locked npm package version is missing"))?;
        let integrity = object
            .get("integrity")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Locked npm package integrity is missing"))?;
        validate_name(package)?;
        anyhow::ensure!(
            valid_version(version),
            "Locked npm package version is invalid"
        );
        anyhow::ensure!(
            integrity.starts_with("sha512-")
                && integrity.len() <= 256
                && integrity
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"+/=-".contains(&byte)),
            "Locked npm package requires a valid SHA-512 integrity digest"
        );
        let url = url::Url::parse(resolved)?;
        anyhow::ensure!(
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.port_or_known_default() == Some(443)
                && url.port().is_none_or(|port| port == 443)
                && url.query().is_none()
                && url.fragment().is_none(),
            "Locked npm package source is not a safe HTTPS registry URL"
        );
        packages.insert(DependencyPackageIdentity {
            package: package.to_owned(),
            version: version.to_owned(),
            source: url.origin().ascii_serialization(),
            integrity: integrity.to_owned(),
        });
    }

    if let Some(entries) = object.get("packages").and_then(Value::as_object) {
        for (path, child) in entries {
            if path.is_empty() {
                continue;
            }
            let Some(name) = name_from_package_path(path) else {
                continue;
            };
            visit(child, Some(&name), packages)?;
        }
    }
    if let Some(entries) = object.get("dependencies").and_then(Value::as_object) {
        for (name, child) in entries {
            visit(child, Some(name), packages)?;
        }
    }
    Ok(())
}

fn name_from_package_path(path: &str) -> Option<String> {
    let tail = path.rsplit_once("node_modules/")?.1;
    let mut parts = tail.split('/');
    let first = parts.next()?;
    if first.starts_with('@') {
        Some(format!("{first}/{}", parts.next()?))
    } else {
        Some(first.to_owned())
    }
}

fn validate_name(name: &str) -> anyhow::Result<()> {
    let decoded = name.replace("%2f", "/").replace("%2F", "/");
    let valid_component = |part: &str| {
        !part.is_empty()
            && part.len() <= 128
            && !part
                .chars()
                .next()
                .is_some_and(|character| matches!(character, '.' | '_' | '-'))
            && !part.ends_with('.')
            && part != "."
            && part != ".."
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-._~".contains(&byte))
    };
    let valid = decoded.strip_prefix('@').map_or_else(
        || !decoded.contains('/') && valid_component(&decoded),
        |scoped| {
            scoped.split_once('/').is_some_and(|(scope, package)| {
                !package.contains('/') && valid_component(scope) && valid_component(package)
            })
        },
    );
    anyhow::ensure!(valid, "Locked npm package name is invalid");
    Ok(())
}

fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 128
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-._+~".contains(&byte))
}
