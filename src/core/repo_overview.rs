//! Bounded, deterministic snapshot of a repository for the planning prompt
//! (§18): README excerpt, shallow file tree, manifests, planning-artifact
//! inventory. Caps everywhere — the goal is orientation, not ingestion.

use std::fs;
use std::path::Path;

const MAX_README_CHARS: usize = 4000;
const MAX_TREE_LINES: usize = 150;
const MAX_PLANNING_FILES: usize = 200;
const TREE_DEPTH: u32 = 2;

/// Directories the tree walk refuses to descend into.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "target",
    "node_modules",
    "vendor",
    "dist",
    "build",
    "out",
    "coverage",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
    ".next",
    ".turbo",
    ".cache",
];

#[derive(Debug, Clone, Default)]
pub struct Overview {
    pub readme: Option<String>,
    pub tree_lines: Vec<String>,
    pub manifests: Vec<String>,
    /// Repo-relative planning artifact paths with sizes (covers imports, §6).
    pub planning_files: Vec<String>,
}

const README_NAMES: &[&str] = &[
    "readme.md",
    "readme.markdown",
    "readme.mdx",
    "readme.txt",
    "readme.rst",
    "readme",
];
const MANIFEST_NAMES: &[&str] = &[
    "package.json",
    "pyproject.toml",
    "setup.py",
    "cargo.toml",
    "pom.xml",
    "go.mod",
    "composer.json",
    "csproj",
    "gradle.build",
    "makefile",
    "dockerfile",
];

pub fn scan(repo: &Path) -> Overview {
    scan_inner(repo, true)
}

/// Scan source code without treating stale embedded artifacts as planning
/// context when a project uses a managed planning repository.
pub fn scan_for_store(
    repo: &Path,
    store: &crate::artifacts::planning_store::PlanningStore,
) -> Overview {
    scan_inner(
        repo,
        store.mode == crate::artifacts::planning_store::StoreMode::LegacyEmbedded,
    )
}

fn scan_inner(repo: &Path, include_legacy_planning: bool) -> Overview {
    Overview {
        readme: find_readme(repo),
        tree_lines: tree_walk(repo, include_legacy_planning),
        manifests: scan_manifests(repo),
        planning_files: if include_legacy_planning {
            list_planning(repo)
        } else {
            Vec::new()
        },
    }
}

/// Case-normalized manifest detection across the repo ROOT (the shapes
/// planners care about; avoids walking vendored trees).
fn scan_manifests(repo: &Path) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(repo) {
        for e in entries.flatten() {
            let lower = e.file_name().to_string_lossy().to_ascii_lowercase();
            if e.path().is_file() && MANIFEST_NAMES.iter().any(|c| *c == lower) {
                found.push(lower);
            }
        }
    }
    found.sort();
    found
}

fn find_readme(repo: &Path) -> Option<String> {
    let entries = fs::read_dir(repo).ok()?;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        if README_NAMES.iter().any(|c| *c == name) && e.path().is_file() {
            let text = fs::read_to_string(e.path()).ok()?;
            return Some(truncate_chars(&text, MAX_README_CHARS));
        }
    }
    None
}

fn truncate_chars(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    let mut out: String = s.chars().take(cap.saturating_sub(16)).collect();
    out.push_str(" …[truncated]");
    out
}

fn is_skipped(name: &str, include_legacy_planning: bool) -> bool {
    SKIP_DIRS.contains(&name)
        || (name.starts_with('.')
            && (!include_legacy_planning
                || name
                    != std::path::Path::new(crate::artifacts::layout::canonical::ROOT)
                        .file_name()
                        .unwrap()
                        .to_string_lossy()))
}

fn tree_walk(root: &Path, include_legacy_planning: bool) -> Vec<String> {
    let mut out = Vec::new();
    walk_into(root, root, 0, &mut out, include_legacy_planning);
    out
}

fn walk_into(
    root: &Path,
    dir: &Path,
    depth: u32,
    out: &mut Vec<String>,
    include_legacy_planning: bool,
) {
    if out.len() >= MAX_TREE_LINES || depth > TREE_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<(String, std::path::PathBuf)> = entries
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(n, _)| !is_skipped(n, include_legacy_planning))
        .collect();
    names.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, path) in names {
        if out.len() >= MAX_TREE_LINES {
            out.push("… (truncated)".into());
            return;
        }
        let rel = path
            .strip_prefix(root)
            .map(|r| r.display().to_string())
            .unwrap_or(name.clone());
        if path.is_dir() {
            out.push(format!("{rel}/"));
            walk_into(root, &path, depth + 1, out, include_legacy_planning);
        } else {
            out.push(rel);
        }
    }
}

/// Everything under Koolade's canonical planning/configuration roots, paths shown relative to the
/// repo root so agents can grab exact filenames for imports (§6).
fn list_planning(repo: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let layout = crate::artifacts::layout::ArtifactLayout::new(repo);
    for base in [layout.planning_root(), layout.config_root()] {
        if base.is_dir() {
            rec_list(&base, repo, &mut out);
        }
    }
    out.sort();
    out
}

fn rec_list(dir: &Path, root: &Path, out: &mut Vec<String>) {
    if out.len() >= MAX_PLANNING_FILES {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries = entries.flatten().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.file_name());
    for e in entries {
        if out.len() >= MAX_PLANNING_FILES {
            break;
        }
        let p = e.path();
        let rel = p
            .strip_prefix(root)
            .map(|r| r.display().to_string())
            .unwrap_or_default();
        if p.is_dir() {
            out.push(format!("{rel}/"));
            rec_list(&p, root, out);
        } else {
            let kb = fs::metadata(&p).map(|m| m.len() / 1024).unwrap_or(0);
            out.push(format!("{rel} ({kb} KB)"));
        }
    }
}

#[cfg(test)]
mod tests;
