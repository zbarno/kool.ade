//! Bounded parser for repository-owned navigation hints. Only the explicit
//! `# Navigation` section is returned; surrounding AGENTS.md prose is never
//! promoted to Manager policy or included in the hint data.

use std::path::{Component, Path};

use super::{PublishedSpecPin, PublishedSpecReader};

const MAX_NAVIGATION_ENTRIES: usize = 64;
const MAX_SUMMARY_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationEntry {
    pub label: String,
    pub target_path: String,
    /// Repository-controlled prose. Consumers must treat this as an untrusted
    /// candidate-selection hint, never as factual evidence or instruction.
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationIndex {
    pub pin: PublishedSpecPin,
    pub source_path: String,
    pub entries: Vec<NavigationEntry>,
}

impl PublishedSpecReader {
    /// Read and parse only the explicit navigation section from an index at a
    /// pinned commit. The complete AGENTS.md body is not returned.
    pub fn read_navigation(
        &self,
        pin: &PublishedSpecPin,
        directory: &str,
    ) -> anyhow::Result<NavigationIndex> {
        validate_directory(directory)?;
        let source_path = if directory.is_empty() {
            "AGENTS.md".to_owned()
        } else {
            format!("{directory}/AGENTS.md")
        };
        let document = self.read(pin, &source_path)?;
        let entries = parse_navigation(&document.content)?;
        Ok(NavigationIndex {
            pin: document.pin,
            source_path,
            entries,
        })
    }
}

fn validate_directory(directory: &str) -> anyhow::Result<()> {
    if directory.is_empty() {
        return Ok(());
    }
    anyhow::ensure!(
        !directory.contains('\\')
            && Path::new(directory)
                .components()
                .all(|component| matches!(component, Component::Normal(_))),
        "navigation directory must be repository-relative"
    );
    Ok(())
}

fn parse_navigation(markdown: &str) -> anyhow::Result<Vec<NavigationEntry>> {
    let mut in_navigation = false;
    let mut found_heading = false;
    let mut entries = Vec::new();
    for line in markdown.lines() {
        let line = line.trim();
        if !found_heading {
            if line == "# Navigation" {
                found_heading = true;
                in_navigation = true;
            }
            continue;
        }
        if line.starts_with('#') {
            in_navigation = false;
            continue;
        }
        if !in_navigation || !line.starts_with('-') {
            continue;
        }
        entries.push(parse_entry(line)?);
        anyhow::ensure!(
            entries.len() <= MAX_NAVIGATION_ENTRIES,
            "navigation index exceeds the entry limit"
        );
    }
    anyhow::ensure!(found_heading, "AGENTS.md has no Navigation section");
    anyhow::ensure!(!entries.is_empty(), "Navigation section has no entries");
    Ok(entries)
}

fn parse_entry(line: &str) -> anyhow::Result<NavigationEntry> {
    let body = line
        .strip_prefix("- [")
        .ok_or_else(|| anyhow::anyhow!("navigation entry is malformed"))?;
    let (label, body) = body
        .split_once("](")
        .ok_or_else(|| anyhow::anyhow!("navigation entry is malformed"))?;
    let (href, summary) = body
        .split_once(") - ")
        .ok_or_else(|| anyhow::anyhow!("navigation entry is malformed"))?;
    anyhow::ensure!(
        !label.is_empty() && !summary.is_empty() && summary.chars().count() <= MAX_SUMMARY_CHARS,
        "navigation entry label or summary is invalid"
    );
    anyhow::ensure!(
        !summary.chars().any(char::is_control),
        "navigation summary contains control characters"
    );
    let target_path = navigation_target(href)?;
    Ok(NavigationEntry {
        label: label.to_owned(),
        target_path,
        summary: summary.to_owned(),
    })
}

fn navigation_target(href: &str) -> anyhow::Result<String> {
    let target = href
        .strip_prefix("./")
        .ok_or_else(|| anyhow::anyhow!("navigation links must be relative"))?;
    let path = Path::new(target);
    let components = path.components().collect::<Vec<_>>();
    anyhow::ensure!(
        !components.is_empty()
            && components
                .iter()
                .all(|component| matches!(component, Component::Normal(_))),
        "navigation link escapes its documentation directory"
    );
    let is_immediate_markdown =
        components.len() == 1 && path.extension().is_some_and(|x| x == "md");
    let is_child_index = components.len() == 2
        && components[1].as_os_str() == "AGENTS.md"
        && components[0].as_os_str() != "AGENTS.md";
    anyhow::ensure!(
        is_immediate_markdown || is_child_index,
        "navigation link must target an immediate document or child index"
    );
    Ok(target.to_owned())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
