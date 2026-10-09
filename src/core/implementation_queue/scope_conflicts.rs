use crate::artifacts::task_docs::TaskDocument;
use std::collections::BTreeSet;

/// Refuse a manual start when its declared file scope overlaps a running task.
pub fn active_scope_conflict(
    docs: &[TaskDocument],
    ticket: &str,
    running: &BTreeSet<String>,
) -> Option<String> {
    let docs = docs
        .iter()
        .filter(|doc| !doc.path.ends_with("/README.md"))
        .collect::<Vec<_>>();
    let candidate = docs.iter().find(|doc| doc.path == ticket)?;
    conflict_for_active_docs(candidate, &docs, running)
}

pub(super) fn conflict_for_active_docs(
    candidate: &TaskDocument,
    docs: &[&TaskDocument],
    running: &BTreeSet<String>,
) -> Option<String> {
    for path in running {
        if path == &candidate.path {
            continue;
        }
        let Some(active) = docs.iter().find(|doc| &doc.path == path) else {
            return Some(format!(
                "{} is waiting because active task {path} has no visible file scope to compare",
                candidate.title
            ));
        };
        if !same_repository(candidate, active) {
            continue;
        }
        let left = FileScope::from_task(candidate);
        let right = FileScope::from_task(active);
        if let Some(overlap) = left.overlap(&right) {
            return Some(format!(
                "Planning-scope heuristic: {} is waiting because its declared affected file scope overlaps active task {} ({overlap}). Actual Git changes are checked again before integration.",
                candidate.title, active.title
            ));
        }
    }
    None
}

fn same_repository(left: &TaskDocument, right: &TaskDocument) -> bool {
    match (repository_id(left), repository_id(right)) {
        (Some(left), Some(right)) => left == right,
        _ => true,
    }
}

fn repository_id(doc: &TaskDocument) -> Option<&str> {
    doc.metadata
        .as_ref()
        .map(|metadata| metadata.repository_id.as_str())
        .or_else(|| {
            doc.text
                .lines()
                .find_map(|line| line.strip_prefix("Repository: "))
        })
}

#[derive(Default)]
struct FileScope {
    paths: BTreeSet<String>,
    names: BTreeSet<String>,
    directories: BTreeSet<String>,
    unknown: bool,
}

impl FileScope {
    fn from_task(doc: &TaskDocument) -> Self {
        let mut scope = Self::default();
        let Some(section) = affected_files_section(&doc.text) else {
            scope.unknown = true;
            return scope;
        };
        for line in section.lines().filter(|line| is_list_item(line)) {
            for token in scope_tokens(line) {
                scope.add_token(token);
            }
        }
        scope.unknown =
            scope.paths.is_empty() && scope.names.is_empty() && scope.directories.is_empty();
        scope
    }

    fn add_token(&mut self, token: &str) {
        let normalized = token.replace('\\', "/");
        let normalized = normalized.trim_matches(['.', '`']);
        if normalized.is_empty() {
            return;
        }
        let lower = normalized.to_ascii_lowercase();
        let segments = lower
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty() && *segment != ".")
            .collect::<Vec<_>>();
        let basename = segments.last().copied().unwrap_or_default();
        if has_file_extension(basename) {
            self.paths.insert(lower.clone());
            self.names.insert(basename.to_owned());
            if let Some((stem, _)) = basename.rsplit_once('.') {
                self.names.insert(stem.to_owned());
            }
        } else if normalized.contains('/') {
            self.directories
                .insert(format!("{}/", lower.trim_matches('/')));
        } else if is_component_name(normalized) {
            self.names.insert(lower);
        }
    }

    fn overlap(&self, other: &Self) -> Option<String> {
        if self.unknown || other.unknown {
            return Some("an unspecified affected-file scope".into());
        }
        if let Some(name) = self.names.intersection(&other.names).next() {
            return Some(name.clone());
        }
        if let Some(path) = self.paths.intersection(&other.paths).next() {
            return Some(path.clone());
        }
        for directory in &self.directories {
            if other.paths.iter().any(|path| path.starts_with(directory))
                || other.directories.iter().any(|candidate| {
                    candidate.starts_with(directory) || directory.starts_with(candidate)
                })
            {
                return Some(directory.clone());
            }
        }
        for directory in &other.directories {
            if self.paths.iter().any(|path| path.starts_with(directory)) {
                return Some(directory.clone());
            }
        }
        None
    }
}

fn affected_files_section(markdown: &str) -> Option<String> {
    let mut in_section = false;
    let mut lines = Vec::new();
    for line in markdown.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if in_section {
                break;
            }
            in_section = heading
                .trim()
                .eq_ignore_ascii_case("Affected files and components");
            continue;
        }
        if in_section {
            lines.push(line);
        }
    }
    in_section.then(|| lines.join("\n"))
}

fn is_list_item(line: &str) -> bool {
    matches!(line.trim_start().chars().next(), Some('-' | '*' | '+'))
}

fn scope_tokens(line: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, character) in line.char_indices() {
        if character.is_ascii_alphanumeric() || "._-/\\".contains(character) {
            start.get_or_insert(index);
        } else if let Some(start) = start.take() {
            tokens.push(&line[start..index]);
        }
    }
    if let Some(start) = start {
        tokens.push(&line[start..]);
    }
    tokens
}

fn has_file_extension(name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    !stem.is_empty()
        && !extension.is_empty()
        && extension.len() <= 8
        && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn is_component_name(value: &str) -> bool {
    value
        .chars()
        .filter(|character| character.is_ascii_uppercase())
        .count()
        > 1
}

#[cfg(test)]
#[path = "scope_conflicts/tests.rs"]
mod tests;
