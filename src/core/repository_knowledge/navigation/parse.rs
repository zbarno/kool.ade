use sha2::{Digest, Sha256};

use super::super::ScanOmissionV1;
use super::NavigationScan;

pub(super) struct Entry {
    pub(super) label: String,
    pub(super) href: String,
    pub(super) purpose: String,
}

pub(super) fn navigation_entries(content: &str) -> Vec<Entry> {
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
        let Some((href, purpose)) = rest.split_once(") - ") else {
            continue;
        };
        if label.trim().is_empty() || purpose.trim().is_empty() {
            continue;
        }
        entries.push(Entry {
            label: label.trim().to_owned(),
            href: href.trim().to_owned(),
            purpose: purpose.trim().to_owned(),
        });
    }
    entries
}

pub(super) fn resolve_target(index: &str, root_dir: &str, entry: &Entry) -> Option<String> {
    let href = entry.href.as_str();
    let relative = href.strip_prefix("./")?;
    if relative.is_empty()
        || relative.contains(['\\', '\0', '?', '#', '%'])
        || relative.starts_with('/')
        || relative.chars().any(char::is_control)
    {
        return None;
    }
    let parts = relative.split('/').collect::<Vec<_>>();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return None;
    }
    let base = index.rsplit_once('/').map_or("", |(parent, _)| parent);
    let target = if base.is_empty() {
        relative.to_owned()
    } else {
        format!("{base}/{relative}")
    };
    if !root_dir.is_empty() && target != root_dir && !target.starts_with(&format!("{root_dir}/")) {
        return None;
    }
    let is_index = target == "AGENTS.md" || target.ends_with("/AGENTS.md");
    if is_index {
        let expected_label = target
            .rsplit_once('/')
            .and_then(|(parent, _)| parent.rsplit('/').next())
            .map_or_else(|| "AGENTS.md".to_owned(), |name| format!("{name}/"));
        return (entry.label == expected_label).then_some(target);
    }
    if !target.ends_with(".md") || target.rsplit('/').next()? != entry.label {
        return None;
    }
    Some(target)
}

pub(super) fn valid_root_index(path: &str) -> Option<String> {
    if !valid_path(path) || !(path == "AGENTS.md" || path.ends_with("/AGENTS.md")) {
        return None;
    }
    Some(
        path.rsplit_once('/')
            .map_or("", |(root, _)| root)
            .to_owned(),
    )
}

pub(super) fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', '\0'])
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(super) fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn sha256(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}

pub(super) fn clip_purpose(value: &str) -> String {
    value.chars().take(super::MAX_PURPOSE_CHARS).collect()
}

pub(super) fn omission(scan: &mut NavigationScan, path: &str, reason: &str) {
    if scan.omissions.len() < super::MAX_OMISSIONS {
        scan.omissions.push(ScanOmissionV1 {
            path: clip_purpose(path),
            reason: reason.into(),
        });
    } else {
        scan.truncated = true;
    }
}
