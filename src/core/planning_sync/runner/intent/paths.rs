use std::{fs, path::Path};

use super::super::super::git;

pub(super) fn read_intent_file(
    repo: &Path,
    head: &str,
    path: &str,
    from_head: bool,
) -> anyhow::Result<Option<Vec<u8>>> {
    if from_head {
        git::read_path(repo, head, path)
    } else {
        read_worktree_file(repo, path)
    }
}

pub(in crate::core::planning_sync::runner) fn read_worktree_file(
    repo: &Path,
    path: &str,
) -> anyhow::Result<Option<Vec<u8>>> {
    let relative = Path::new(path);
    anyhow::ensure!(
        !path.is_empty()
            && !path.starts_with('/')
            && !path.contains('\\')
            && relative
                .components()
                .all(|component| matches!(component, std::path::Component::Normal(_))),
        "planning sync refuses unsafe record paths"
    );
    let components = relative.components().collect::<Vec<_>>();
    let mut cursor = repo.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let std::path::Component::Normal(part) = component else {
            anyhow::bail!("planning sync refuses unsafe record paths");
        };
        cursor.push(part);
        let metadata = match fs::symlink_metadata(&cursor) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "planning sync refuses symlink records"
        );
        if index + 1 == components.len() {
            return if metadata.is_file() {
                Ok(Some(fs::read(cursor)?))
            } else {
                Ok(None)
            };
        }
        if !metadata.is_dir() {
            return Ok(None);
        }
    }
    Ok(None)
}

pub(super) fn record_id_for_path(path: &str) -> Option<String> {
    let (collection, file) = path.rsplit_once('/')?;
    if !matches!(
        collection,
        "state/work"
            | "state/workflow"
            | "state/items"
            | "state/tasks"
            | "planning/items"
            | "state/questions"
            | "state/readiness"
            | "planning/questions"
    ) {
        return None;
    }
    let uid = file.rsplit_once('.')?.0;
    let valid_extension = (matches!(collection, "planning/items" | "planning/questions")
        && file.ends_with(".md"))
        || (!matches!(collection, "planning/items" | "planning/questions")
            && file.ends_with(".json"));
    let parsed = uuid::Uuid::parse_str(uid).ok()?;
    (valid_extension && parsed.hyphenated().to_string() == uid).then(|| uid.to_owned())
}

pub(super) fn is_sync_path(path: &str) -> bool {
    let relative = Path::new(path);
    let safe = !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !relative.is_absolute()
        && relative
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_)));
    safe && (path.starts_with("state/") || path.starts_with("planning/"))
        && !is_legacy_authoritative_path(path)
}

pub(super) fn is_legacy_authoritative_path(path: &str) -> bool {
    matches!(
        path,
        "state/work.json"
            | "state/workflow.json"
            | "planning/open-items.md"
            | "planning/resolved-items.json"
    )
}
