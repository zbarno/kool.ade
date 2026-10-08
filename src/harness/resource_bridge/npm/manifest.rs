//! Bounded discovery of declared npm dependencies when a project has no lockfile.
use serde_json::Value;
use std::{collections::VecDeque, fs, path::Path};

const MAX_MANIFESTS: usize = 256;
const MAX_MANIFEST_BYTES: u64 = 2 * 1024 * 1024;
const MAX_DEPTH: usize = 20;

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
