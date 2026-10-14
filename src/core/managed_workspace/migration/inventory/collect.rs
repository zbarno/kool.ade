use std::{fs, path::Path};

use sha2::{Digest, Sha256};

use super::{Artifact, ArtifactMapping, Kind};

pub(crate) fn collect(code_root: &Path, repository_id: &str) -> anyhow::Result<Vec<Artifact>> {
    let root = code_root.join(crate::artifacts::layout::canonical::ROOT);
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("legacy planning root does not exist: {}", root.display())
        }
        Err(error) => return Err(error.into()),
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "legacy planning root must be a real directory"
        ),
    }
    let mut paths = Vec::new();
    walk(&root, &root, &mut paths)?;
    paths.sort();
    let mut artifacts = Vec::with_capacity(paths.len());
    let mut destinations = std::collections::BTreeMap::<String, String>::new();
    for relative in paths {
        let bytes = fs::read(root.join(&relative))?;
        let (kind, disposition, mut target_path) = mapping(&relative);
        if kind == Kind::Private && repository_id != "root" {
            target_path = target_path.map(|path| {
                format!(
                    "operator-local/repositories/{repository_id}/{}",
                    path.trim_start_matches("operator-local/")
                )
            });
        }
        if kind == Kind::Shared
            && let Some(previous) =
                destinations.insert(target_path.clone().unwrap(), relative.clone())
        {
            anyhow::bail!("planning migration destination collision: {previous} and {relative}");
        }
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let report_sha = (kind != Kind::Private).then(|| sha256.clone());
        artifacts.push(Artifact {
            relative: relative.clone(),
            bytes,
            sha256,
            mapping: ArtifactMapping {
                repository_id: Some(repository_id.to_owned()),
                source_path: format!(".koolade-packet/{relative}"),
                sha256: report_sha,
                disposition: disposition.to_owned(),
                target_path,
            },
            kind,
        });
    }
    Ok(artifacts)
}

pub(crate) fn collect_optional(
    code_root: &Path,
    repository_id: &str,
) -> anyhow::Result<Vec<Artifact>> {
    let root = code_root.join(crate::artifacts::layout::canonical::ROOT);
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "legacy planning root must be a real directory: {}",
            root.display()
        ),
    }
    collect(code_root, repository_id)
}

pub(super) fn walk(root: &Path, directory: &Path, out: &mut Vec<String>) -> anyhow::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "linked legacy artifact cannot be migrated safely: {}",
            path.strip_prefix(root)?.display()
        );
        if metadata.is_dir() {
            walk(root, &path, out)?;
        } else {
            anyhow::ensure!(metadata.is_file(), "unsupported legacy artifact type");
            let relative = path
                .strip_prefix(root)?
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("legacy artifact path is not UTF-8"))?
                .replace('\\', "/");
            out.push(relative);
        }
    }
    Ok(())
}

pub(super) fn mapping(relative: &str) -> (Kind, &'static str, Option<String>) {
    use Kind::*;
    if relative == "manifest.json" {
        return (
            ReplacedManifest,
            "replaced-by-workspace-manifest",
            Some("manifest.json".into()),
        );
    }
    if relative == "config/repositories.json" {
        return (
            ReplacedManifest,
            "rewritten-from-root-repository-registry",
            Some(relative.into()),
        );
    }
    if is_normalized_input(relative) {
        return (
            Normalized,
            "normalized-into-per-record-layout",
            Some(normalized_target(relative)),
        );
    }
    if is_shared(relative) {
        return (
            Shared,
            "copied-to-planning-repository",
            Some(relative.into()),
        );
    }
    let local = if relative == "config/mcp.json" {
        "operator-local/config/mcp.json".to_owned()
    } else if let Some(rest) = relative.strip_prefix("implementation/") {
        format!("operator-local/implementation/{rest}")
    } else if relative == "implementation" {
        "operator-local/implementation".into()
    } else if relative == "state/time-ledger.log" {
        "operator-local/state/time-ledger.log".into()
    } else if let Some(rest) = relative.strip_prefix("state/locks/") {
        format!("operator-local/state/locks/{rest}")
    } else if let Some(rest) = relative.strip_prefix("conversations/") {
        format!("operator-local/conversations/{rest}")
    } else {
        format!("operator-local/unclassified/{relative}")
    };
    (Private, "copied-to-operator-local-storage", Some(local))
}

fn is_shared(path: &str) -> bool {
    path.starts_with("planning/")
        || path == "config/project.md"
        || path == "config/repositories.json"
        || path == "state/cancelled-work.json"
        || path.starts_with("state/tasks/")
}

fn is_normalized_input(path: &str) -> bool {
    matches!(
        path,
        "state/work.json"
            | "state/workflow.json"
            | "planning/open-items.md"
            | "planning/resolved-items.json"
    ) || path.starts_with("state/work/")
        || path.starts_with("state/workflow/")
        || path.starts_with("state/items/")
        || path.starts_with("planning/items/")
        || path.starts_with("state/questions/")
        || path.starts_with("state/readiness/")
        || path.starts_with("planning/questions/")
}

fn normalized_target(path: &str) -> String {
    match path {
        "state/work.json" => "state/work/<work-uid>.json".into(),
        "state/workflow.json" => "state/workflow/<feature-uid>.json".into(),
        "planning/open-items.md" | "planning/resolved-items.json" => {
            "planning/items/<item-uid>.md + state/items/<item-uid>.json".into()
        }
        _ if path.starts_with("state/work/") => path.to_owned(),
        _ if path.starts_with("state/workflow/") => path.to_owned(),
        _ if path.starts_with("state/items/") => path.to_owned(),
        _ if path.starts_with("planning/items/") => path.to_owned(),
        _ if path.starts_with("state/questions/") => path.to_owned(),
        _ if path.starts_with("state/readiness/") => path.to_owned(),
        _ if path.starts_with("planning/questions/") => path.to_owned(),
        _ => "state/work/<work-uid>.json".into(),
    }
}
