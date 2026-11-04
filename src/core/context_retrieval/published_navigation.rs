//! Bounded discovery of published documents through untrusted AGENTS.md
//! navigation sections. No instructions outside that section are returned.

use crate::core::published_specs::{PublishedSpecPin, PublishedSpecReader};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Component, Path};

const ROOT_INDEX: &str = "docs/koolade/AGENTS.md";
const MAX_INDEXES: usize = 8;
const MAX_DEPTH: usize = 4;
const MAX_INDEX_BYTES: usize = 32 * 1024;
const MAX_CANDIDATES: usize = 64;
const MAX_SUMMARY_BYTES: usize = 400;

#[derive(Debug, Clone)]
pub(super) struct PublishedCandidate {
    pub(super) id: String,
    pub(super) path: String,
    pub(super) summary: String,
}

struct IndexRequest {
    path: String,
    depth: usize,
}

struct NavigationEntry {
    label: String,
    href: String,
    summary: String,
}

pub(super) fn catalog(
    reader: &PublishedSpecReader,
    pin: &PublishedSpecPin,
) -> BTreeMap<String, PublishedCandidate> {
    let mut result = BTreeMap::new();
    let mut pending = VecDeque::from([IndexRequest {
        path: ROOT_INDEX.into(),
        depth: 0,
    }]);
    let mut visited = BTreeSet::new();

    while let Some(request) = pending.pop_front() {
        if visited.len() >= MAX_INDEXES
            || request.depth > MAX_DEPTH
            || !visited.insert(request.path.clone())
        {
            continue;
        }
        let Ok(index) = reader.read(pin, &request.path) else {
            continue;
        };
        if index.content.len() > MAX_INDEX_BYTES {
            continue;
        }
        for entry in navigation_entries(&index.content) {
            let Some(path) = resolved_target(&request.path, &entry) else {
                continue;
            };
            if path.ends_with("/AGENTS.md") {
                pending.push_back(IndexRequest {
                    path,
                    depth: request.depth + 1,
                });
                continue;
            }
            if result.len() >= MAX_CANDIDATES {
                break;
            }
            let id = format!("published:{path}");
            result.entry(id.clone()).or_insert(PublishedCandidate {
                id,
                path,
                summary: clip_summary(&entry.summary),
            });
        }
        if result.len() >= MAX_CANDIDATES {
            break;
        }
    }
    result
}

fn navigation_entries(content: &str) -> Vec<NavigationEntry> {
    let mut in_navigation = false;
    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line == "# Navigation" {
            in_navigation = true;
            continue;
        }
        if in_navigation && line.starts_with("# ") {
            break;
        }
        if !in_navigation {
            continue;
        }
        let Some(link) = line.strip_prefix("- [") else {
            continue;
        };
        let Some((label, rest)) = link.split_once("](") else {
            continue;
        };
        let Some((href, summary)) = rest.split_once(") - ") else {
            continue;
        };
        if label.is_empty() || href.is_empty() || summary.trim().is_empty() {
            continue;
        }
        entries.push(NavigationEntry {
            label: label.to_owned(),
            href: href.to_owned(),
            summary: summary.trim().to_owned(),
        });
    }
    entries
}

fn resolved_target(index_path: &str, entry: &NavigationEntry) -> Option<String> {
    let href = entry.href.strip_prefix("./")?;
    if href.contains('\\') || href.contains('\0') {
        return None;
    }
    let relative = Path::new(href);
    let components = relative.components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    let index_dir = Path::new(index_path).parent()?;
    let resolved = index_dir.join(relative);
    if !resolved.starts_with("docs/koolade") {
        return None;
    }
    let is_index = resolved.file_name().is_some_and(|name| name == "AGENTS.md");
    if is_index {
        if components.len() != 2
            || entry.label != format!("{}/", components[0].as_os_str().to_string_lossy())
        {
            return None;
        }
        return normalized(&resolved);
    }
    if components.len() != 1
        || resolved
            .extension()
            .is_none_or(|extension| extension != "md")
        || resolved.file_name().is_some_and(|name| name == "AGENTS.md")
        || entry.label != resolved.file_name()?.to_string_lossy()
    {
        return None;
    }
    normalized(&resolved)
}

fn normalized(path: &Path) -> Option<String> {
    path.components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

fn clip_summary(summary: &str) -> String {
    let mut end = summary.len().min(MAX_SUMMARY_BYTES);
    while !summary.is_char_boundary(end) {
        end -= 1;
    }
    summary[..end].to_owned()
}

#[cfg(test)]
#[path = "published_navigation/tests.rs"]
mod tests;
