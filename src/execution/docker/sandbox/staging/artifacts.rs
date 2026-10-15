use std::{collections::BTreeMap, path::PathBuf};

use crate::execution::contract::ArtifactManifestEntryV1;

use super::archive::{Fingerprint, ReturnedEntry};

pub(super) fn collect(
    entries: &BTreeMap<PathBuf, ReturnedEntry>,
    paths: &[String],
    byte_limit: u64,
) -> anyhow::Result<Vec<ArtifactManifestEntryV1>> {
    anyhow::ensure!(paths.len() <= 128, "too many requested artifacts");
    let mut unique = std::collections::BTreeSet::new();
    let mut total = 0_u64;
    let mut artifacts = Vec::with_capacity(paths.len());
    for path in paths {
        anyhow::ensure!(
            unique.insert(path),
            "requested artifacts contain duplicates"
        );
        let returned = entries
            .get(&PathBuf::from(path))
            .ok_or_else(|| anyhow::anyhow!("requested artifact is missing"))?;
        let Fingerprint::File { sha256, size, .. } = &returned.fingerprint else {
            anyhow::bail!("requested artifact is not a regular file");
        };
        total = total
            .checked_add(*size)
            .ok_or_else(|| anyhow::anyhow!("artifact byte count overflowed"))?;
        anyhow::ensure!(
            total <= byte_limit,
            "artifacts exceed the synchronization limit"
        );
        let artifact = ArtifactManifestEntryV1 {
            relative_path: path.clone(),
            size_bytes: *size,
            sha256: sha256.clone(),
            media_type: media_type(path).to_owned(),
        };
        artifact.validate()?;
        artifacts.push(artifact);
    }
    Ok(artifacts)
}

fn media_type(path: &str) -> &'static str {
    match PathBuf::from(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "wasm" => "application/wasm",
        "txt" | "md" => "text/plain",
        _ => "application/octet-stream",
    }
}
