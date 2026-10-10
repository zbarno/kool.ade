//! Feature identifiers, safe paths, and the application-maintained index.
use crate::artifacts::planning_store::PlanningRoot;
use std::path::Path;

pub fn valid_feature_id(id: &str) -> bool {
    id.strip_prefix('F')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        || id
            .strip_prefix("CHG-")
            .is_some_and(|digits| digits.len() >= 3 && digits.bytes().all(|b| b.is_ascii_digit()))
}

pub(super) fn directory_feature_id(name: &str) -> Option<&str> {
    let separator = name.find('-')?;
    let mut id = &name[..separator];
    if id == "CHG" {
        let digits = name[separator + 1..].split_once('-')?.0;
        id = name.get(..4 + digits.len())?;
    }
    valid_feature_id(id).then_some(id)
}

fn feature_number(id: &str) -> Option<u32> {
    id.strip_prefix('F')
        .or_else(|| id.strip_prefix("CHG-"))?
        .parse()
        .ok()
}

pub(super) fn regular(path: &Path) -> anyhow::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_file() && !meta.file_type().is_symlink(),
                "{} is not a regular file",
                path.display()
            );
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn real_dir(path: &Path) -> anyhow::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            anyhow::ensure!(
                meta.is_dir() && !meta.file_type().is_symlink(),
                "{} is not a real directory",
                path.display()
            );
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub fn next_feature_id<R: PlanningRoot + ?Sized>(repo: &R) -> String {
    let layout = repo.planning_layout();
    let mut maximum = 0u32;
    if let Ok(entries) = std::fs::read_dir(layout.changes_root()) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && let Some(number) = directory_feature_id(name).and_then(feature_number)
            {
                maximum = maximum.max(number);
            }
        }
    }
    let store = repo.planning_store();
    let git_root = repo
        .code_repository_root()
        .unwrap_or_else(|| layout.root().to_path_buf());
    let history_path = if store.mode == crate::artifacts::planning_store::StoreMode::LegacyEmbedded
    {
        crate::artifacts::layout::canonical::CHANGES
    } else {
        crate::artifacts::planning_store::paths::CHANGES
    };
    if let Ok(output) = std::process::Command::new("git")
        .args([
            "log",
            "--all",
            "--name-only",
            "--pretty=format:",
            "--",
            history_path,
        ])
        .current_dir(git_root)
        .output()
        && output.status.success()
    {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if let Some(name) = line.strip_prefix(&format!("{history_path}/"))
                && let Some(number) = directory_feature_id(name).and_then(feature_number)
            {
                maximum = maximum.max(number);
            }
        }
    }
    format!("F{}", maximum + 1)
}

pub fn refreshed_index<R: PlanningRoot + ?Sized>(
    repo: &R,
    updates: &[(String, String)],
) -> anyhow::Result<String> {
    let index =
        String::from_utf8(repo.read_planning_path(&repo.planning_layout().product_index())?)?;
    refreshed_index_from(repo, &index, updates)
}

pub fn refreshed_index_from<R: PlanningRoot + ?Sized>(
    repo: &R,
    index: &str,
    updates: &[(String, String)],
) -> anyhow::Result<String> {
    let manifest = super::document_updates::updated_manifest(repo, updates)?;
    let layout = repo.planning_layout();
    let title = index
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .unwrap_or("Product — Living Technical Specification");
    let intro = index
        .lines()
        .skip(1)
        .take_while(|line| {
            !line.starts_with("## Modules")
                && !line.starts_with("## Product modules")
                && !line.starts_with("## Active features")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned();
    let mut entries = super::active_features::active_feature_directories(repo);
    for (id, content) in updates.iter().filter(|(id, _)| id.starts_with("feature:")) {
        let path = super::document_updates::document_path_for_update(repo, id, content)?;
        let name = path
            .parent()
            .and_then(|dir| dir.file_name())
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid feature path"))?;
        entries.retain(|entry| directory_feature_id(entry) != directory_feature_id(name));
        let status = crate::domain::ChangeMetadata::require_markdown(content)?.status;
        if !status.is_terminal() {
            entries.push(name.to_owned());
        }
    }
    entries.sort();
    entries.dedup();
    let mut result = format!("# {title}\n\n{intro}\n\n## Product modules\n\n");
    for module in &manifest.modules {
        let core = module
            .core_concept
            .map(|concept| format!(" — required: {}", concept.title()))
            .unwrap_or_default();
        result.push_str(&format!("- [{}]({}){core}\n", module.title, module.path));
    }
    result.push_str("\n## Active features\n\n");
    if entries.is_empty() {
        result.push_str("None.\n");
    } else {
        for name in entries {
            let link = layout
                .change_link(&name)
                .ok_or_else(|| anyhow::anyhow!("Invalid feature link"))?;
            result.push_str(&format!("- [`{name}`]({link})\n"));
        }
    }
    Ok(result)
}
