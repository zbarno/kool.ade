//! Bounded discovery of declared npm dependencies when a project has no lockfile.
use serde_json::Value;
use std::{collections::VecDeque, fs, io::Read, path::Path};

const MAX_MANIFESTS: usize = 256;
const MAX_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
const MAX_DEPTH: usize = 20;
const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;

pub(super) fn dependency_inputs(root: &Path) -> anyhow::Result<Vec<(String, Value)>> {
    let root = root.canonicalize()?;
    let mut pending = VecDeque::from([(root.clone(), 0_usize)]);
    let mut inputs = Vec::new();
    let mut scanned = 0_usize;
    let mut total_bytes = 0_u64;
    while let Some((directory, depth)) = pending.pop_front() {
        anyhow::ensure!(
            depth <= MAX_DEPTH,
            "project tree exceeds the npm manifest scan depth"
        );
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                anyhow::ensure!(
                    entry.file_name().to_str() != Some("package.json"),
                    "npm package.json symlinks are not supported for dependency authorization"
                );
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                if !ignored_directory(&entry.file_name()) {
                    pending.push_back((path, depth + 1));
                }
                continue;
            }
            if path.file_name().and_then(|name| name.to_str()) != Some("package.json") {
                continue;
            }
            anyhow::ensure!(
                file_type.is_file(),
                "npm package.json must be a regular file"
            );
            scanned += 1;
            anyhow::ensure!(
                scanned <= MAX_MANIFESTS,
                "project contains too many npm manifests"
            );
            let metadata = fs::metadata(&path)?;
            anyhow::ensure!(
                metadata.len() <= MAX_MANIFEST_BYTES,
                "npm package.json exceeds the 2 MiB scan limit"
            );
            let mut bytes = Vec::new();
            fs::File::open(&path)?
                .take(MAX_MANIFEST_BYTES + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() as u64 <= MAX_MANIFEST_BYTES,
                "npm package.json exceeds the 2 MiB scan limit"
            );
            total_bytes = total_bytes.saturating_add(bytes.len() as u64);
            anyhow::ensure!(
                total_bytes <= MAX_INPUT_BYTES,
                "npm dependency manifests exceed the scan budget"
            );
            let input = dependency_input_from_bytes(&bytes)?;
            if !input.as_object().is_some_and(serde_json::Map::is_empty) {
                let relative = path
                    .strip_prefix(&root)?
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                inputs.push((relative, input));
            }
        }
    }
    inputs.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(inputs)
}

pub(super) fn dependency_input_from_bytes(bytes: &[u8]) -> anyhow::Result<Value> {
    let manifest: Value = serde_json::from_slice(bytes)?;
    let Some(object) = manifest.as_object() else {
        anyhow::bail!("npm package.json must contain a JSON object");
    };
    let mut inputs = serde_json::Map::new();
    for field in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if let Some(value) = object.get(field) {
            let Some(dependencies) = value.as_object() else {
                anyhow::bail!("npm package.json {field} must be an object");
            };
            if !dependencies.is_empty() {
                inputs.insert(field.to_owned(), value.clone());
            }
        }
    }
    for field in [
        "peerDependenciesMeta",
        "overrides",
        "workspaces",
        "bundleDependencies",
        "bundledDependencies",
    ] {
        if let Some(value) = object.get(field)
            && !value.as_object().is_some_and(serde_json::Map::is_empty)
            && !value.as_array().is_some_and(Vec::is_empty)
        {
            inputs.insert(field.to_owned(), value.clone());
        }
    }
    Ok(Value::Object(inputs))
}

fn ignored_directory(name: &std::ffi::OsStr) -> bool {
    matches!(
        name.to_str(),
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
    )
}

pub(super) fn has_declared_dependencies(root: &Path) -> anyhow::Result<bool> {
    let root = root.canonicalize()?;
    let mut pending = VecDeque::from([(root, 0_usize)]);
    let mut manifests = 0;
    while let Some((directory, depth)) = pending.pop_front() {
        anyhow::ensure!(
            depth <= MAX_DEPTH,
            "project tree exceeds the npm manifest scan depth"
        );
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
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
                    pending.push_back((path, depth + 1));
                }
                continue;
            }
            if path.file_name().and_then(|name| name.to_str()) != Some("package.json") {
                continue;
            }
            manifests += 1;
            anyhow::ensure!(
                manifests <= MAX_MANIFESTS,
                "project contains too many npm manifests"
            );
            let metadata = fs::metadata(&path)?;
            anyhow::ensure!(
                metadata.len() <= MAX_MANIFEST_BYTES,
                "npm package.json exceeds the 2 MiB scan limit"
            );
            let manifest: Value = serde_json::from_slice(&fs::read(path)?)?;
            let Some(object) = manifest.as_object() else {
                anyhow::bail!("npm package.json must contain a JSON object");
            };
            for field in [
                "dependencies",
                "devDependencies",
                "optionalDependencies",
                "peerDependencies",
            ] {
                if let Some(dependencies) = object.get(field) {
                    let Some(dependencies) = dependencies.as_object() else {
                        anyhow::bail!("npm package.json {field} must be an object");
                    };
                    if !dependencies.is_empty() {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
}
