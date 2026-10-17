use super::{ExecutionWorkspaceEntryKindV1, ExecutionWorkspaceEntryV1};

pub(super) fn validate(entries: &[ExecutionWorkspaceEntryV1]) -> Result<(), String> {
    if entries.len() > 100_000
        || entries
            .iter()
            .try_fold(0_u64, |sum, entry| sum.checked_add(entry.size_bytes))
            .is_none_or(|bytes| bytes > 512 * 1024 * 1024)
    {
        return Err("execution workspace baseline exceeds its limit".into());
    }
    let path_bytes = entries.iter().try_fold(0_usize, |sum, entry| {
        sum.checked_add(entry.relative_path.len())
    });
    if path_bytes.is_none_or(|bytes| bytes > 16 * 1024 * 1024) {
        return Err("execution workspace baseline paths exceed their limit".into());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut kinds = std::collections::BTreeMap::new();
    for entry in entries {
        let path = std::path::Path::new(&entry.relative_path);
        if entry.relative_path.is_empty()
            || entry.relative_path.len() > 4096
            || path.is_absolute()
            || entry.relative_path.contains('\\')
            || entry.relative_path.chars().any(char::is_control)
            || !path
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
            || entry.relative_path == ".git"
            || entry.relative_path.starts_with(".git/")
            || !seen.insert(&entry.relative_path)
            || entry.mode > 0o777
            || match entry.kind {
                ExecutionWorkspaceEntryKindV1::Directory => {
                    entry.sha256.is_some() || entry.size_bytes != 0
                }
                ExecutionWorkspaceEntryKindV1::File => {
                    !entry.sha256.as_deref().is_some_and(|sha| {
                        sha.len() == 64 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
                    })
                }
            }
        {
            return Err("execution workspace baseline entry is invalid".into());
        }
        kinds.insert(entry.relative_path.as_str(), entry.kind);
    }
    for entry in entries {
        let path = std::path::Path::new(&entry.relative_path);
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && kinds.get(parent.to_str().unwrap_or_default())
                != Some(&ExecutionWorkspaceEntryKindV1::Directory)
        {
            return Err("execution workspace baseline parent directory is missing".into());
        }
    }
    Ok(())
}
