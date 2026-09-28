use std::{
    fs,
    path::{Component, Path},
};

use super::Entry;

pub(super) fn migrate_workflow(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut workflow: crate::core::workflow::Workflow = serde_json::from_slice(bytes)
        .map_err(|error| anyhow::anyhow!("Cannot migrate planner workflow state: {error}"))?;
    let mut changed = false;
    for batch in &mut workflow.task_batches {
        let directory = &mut batch.directory;
        if let Some(suffix) = directory.strip_prefix("planning/tasks/") {
            let old = format!("planning/tasks/{suffix}/placeholder.md");
            validate_relative(&old)?;
            *directory = format!("{}/{suffix}", crate::artifacts::layout::canonical::TASKS);
            changed = true;
        } else if let Some(suffix) = directory.strip_prefix("planning/features/") {
            let old = format!("planning/features/{suffix}/placeholder.md");
            validate_relative(&old)?;
            *directory = format!("{}/{suffix}", crate::artifacts::layout::canonical::CHANGES);
            changed = true;
        }
    }
    if changed {
        Ok(serde_json::to_vec_pretty(&workflow)?)
    } else {
        Ok(bytes.to_vec())
    }
}

pub(super) fn collect(repo: &Path, path: &Path, out: &mut Vec<Entry>) -> anyhow::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    anyhow::ensure!(
        !metadata.file_type().is_symlink(),
        "Linked legacy artifact {} cannot be migrated safely",
        path.display()
    );
    if metadata.is_file() {
        out.push(entry(repo, path)?);
        return Ok(());
    }
    anyhow::ensure!(
        metadata.is_dir(),
        "Unsupported legacy artifact {}",
        path.display()
    );
    let mut children = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    children.sort_by_key(|child| child.file_name());
    for child in children {
        collect(repo, &child.path(), out)?;
    }
    Ok(())
}

pub(super) fn entry(repo: &Path, source_path: &Path) -> anyhow::Result<Entry> {
    let source = source_path
        .strip_prefix(repo)?
        .components()
        .map(|component| {
            component.as_os_str().to_str().ok_or_else(|| {
                anyhow::anyhow!(
                    "Legacy artifact path is not UTF-8: {}",
                    source_path.display()
                )
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?
        .join("/");
    let target = destination(&source)?;
    Ok(Entry {
        source_bytes: fs::read(source_path)?,
        target_bytes: fs::read(source_path)?,
        source,
        target,
    })
}

pub(in crate::artifacts::migration) fn destination(source: &str) -> anyhow::Result<String> {
    use crate::artifacts::layout::{canonical, legacy};
    if source == crate::artifacts::layout::previous::WORK {
        return Ok(canonical::WORK.into());
    }
    if source == legacy::SPECIFICATION {
        return Ok(canonical::LEGACY_SPEC_ARCHIVE.into());
    }
    if source == legacy::ROOT_SPECIFICATION {
        return Ok(format!("{}/SPECIFICATION.md", canonical::ARCHIVE));
    }
    if let Some(rest) = source.strip_prefix(&format!("{}/", legacy::FEATURES)) {
        return Ok(format!("{}/{rest}", canonical::CHANGES));
    }
    if let Some(rest) = source.strip_prefix(&format!("{}/", legacy::ARCHIVE)) {
        return Ok(format!("{}/{rest}", canonical::ARCHIVE));
    }
    if source == legacy::PLANNING {
        anyhow::bail!("The legacy planning root contains no migratable file path");
    }
    if let Some(rest) = source.strip_prefix(&format!("{}/", legacy::PLANNING)) {
        return Ok(format!("{}/{rest}", canonical::PLANNING));
    }
    if source == legacy::PROJECT_CONFIG {
        return Ok(canonical::PROJECT_CONFIG.into());
    }
    if source == legacy::MCP_CONFIG {
        return Ok(canonical::MCP_CONFIG.into());
    }
    if source == legacy::WORKFLOW {
        return Ok(canonical::WORKFLOW.into());
    }
    if source == legacy::PROJECT_MANIFEST {
        return Ok(canonical::PROJECT_MANIFEST.into());
    }
    if let Some(rest) = source.strip_prefix(&format!("{}/", legacy::CONFIG)) {
        return Ok(format!("{}/legacy-planner/{rest}", canonical::ARCHIVE));
    }
    if let Some(rest) = source.strip_prefix(&format!("{}/", legacy::ADR)) {
        let name = Path::new(rest)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if name.starts_with("implement-") {
            return Ok(format!(
                "{}/{}/{}",
                canonical::ARCHIVE,
                "implementation-decisions",
                rest
            ));
        }
        return Ok(format!("{}/{rest}", canonical::DECISIONS));
    }
    anyhow::bail!("Unsupported legacy artifact path {source}")
}

pub(in crate::artifacts::migration) fn relocated_ticket_path(ticket: &str) -> Option<String> {
    let suffix = ticket.strip_prefix(&format!("{}/", crate::artifacts::layout::legacy::TASKS))?;
    let legacy_path = format!("{}/{suffix}", crate::artifacts::layout::legacy::TASKS);
    validate_relative(&legacy_path).ok()?;
    Some(format!(
        "{}/{suffix}",
        crate::artifacts::layout::canonical::TASKS
    ))
}

pub(super) fn validate_relative(relative: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !relative.is_empty()
            && Path::new(relative)
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "Invalid migration path {relative}"
    );
    Ok(())
}

pub(super) fn check_destination(
    repo: &Path,
    relative: &str,
    expected: &[u8],
) -> anyhow::Result<()> {
    let path = repo.join(relative);
    let mut ancestor = repo.to_path_buf();
    let components = Path::new(relative).components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        ancestor.push(component.as_os_str());
        match fs::symlink_metadata(&ancestor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                anyhow::bail!(
                    "Migration target path {} contains a symbolic link",
                    ancestor.display()
                )
            }
            Ok(meta) if index + 1 < components.len() && !meta.is_dir() => {
                anyhow::bail!(
                    "Migration target parent {} is not a directory",
                    ancestor.display()
                )
            }
            Ok(meta) if index + 1 == components.len() => {
                anyhow::ensure!(
                    meta.is_file(),
                    "Migration target {} is not a regular file",
                    relative
                );
                anyhow::ensure!(
                    fs::read(&path)? == expected,
                    "Migration conflict: legacy artifact differs from existing target {}; both were preserved",
                    relative
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
