use std::path::{Component, Path, PathBuf};

pub(super) fn is_quality_heading(heading: &str) -> bool {
    if (heading.contains("entry point") || heading.contains("available command"))
        && !heading.contains("required")
    {
        return false;
    }
    [
        "quality",
        "verification",
        "test",
        "check",
        "validation",
        "gate",
    ]
    .iter()
    .any(|word| heading.contains(word))
}

pub(super) fn scope_applies_to_changes(
    line: &str,
    instruction_directory: &Path,
    changed_paths: &[PathBuf],
) -> bool {
    let lower = line.to_ascii_lowercase();
    let Some(required_at) = lower.find("required after") else {
        return true;
    };
    let after_required = &line[required_at + "required after".len()..];
    let lower_after_required = &lower[required_at + "required after".len()..];
    let Some(under) = lower_after_required.find("under") else {
        return true;
    };
    let suffix = &after_required[under + "under".len()..];
    let Some(start) = suffix.find('`') else {
        return true;
    };
    let rest = &suffix[start + 1..];
    let Some(end) = rest.find('`') else {
        return true;
    };
    let scope = Path::new(rest[..end].trim());
    let Some(scope_components) = normal_components(scope) else {
        return false;
    };
    if scope_components.is_empty() || changed_paths.is_empty() {
        return changed_paths.is_empty() && scope_components.is_empty();
    }
    let mut scope_root = instruction_directory.to_path_buf();
    let prefix = super::commands::inline_commands(line)
        .iter()
        .find_map(|command| command_prefix_path(command));
    if let Some(prefix) = prefix {
        let Some(prefix_components) = normal_components(&prefix) else {
            return false;
        };
        scope_root.push(&prefix);
        let overlap = (1..=prefix_components.len().min(scope_components.len()))
            .rev()
            .find(|count| {
                prefix_components[prefix_components.len() - *count..] == scope_components[..*count]
            })
            .unwrap_or(0);
        for component in scope_components.iter().skip(overlap) {
            scope_root.push(component);
        }
    } else {
        for component in scope_components {
            scope_root.push(component);
        }
    }
    changed_paths
        .iter()
        .any(|changed| changed.starts_with(&scope_root))
}

fn normal_components(path: &Path) -> Option<Vec<std::ffi::OsString>> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => components.push(part.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(components)
}

fn command_prefix_path(command: &str) -> Option<PathBuf> {
    let tokens = command.split_whitespace().collect::<Vec<_>>();
    tokens.iter().enumerate().find_map(|(index, token)| {
        if let Some(prefix) = token.strip_prefix("--prefix=") {
            return Some(PathBuf::from(prefix.trim_matches(['\'', '"'])));
        }
        (*token == "--prefix")
            .then(|| tokens.get(index + 1).copied())
            .flatten()
            .map(|prefix| PathBuf::from(prefix.trim_matches(['\'', '"'])))
    })
}
