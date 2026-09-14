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
    Overview {
        readme: find_readme(repo),
        tree_lines: tree_walk(repo),
        manifests: scan_manifests(repo),
        planning_files: list_planning(repo),
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

fn is_skipped(name: &str) -> bool {
    SKIP_DIRS.contains(&name) || (name.starts_with('.') && name != ".planner")
}

fn tree_walk(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk_into(root, root, 0, &mut out);
    out
}

fn walk_into(root: &Path, dir: &Path, depth: u32, out: &mut Vec<String>) {
    if out.len() >= MAX_TREE_LINES || depth > TREE_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<(String, std::path::PathBuf)> = entries
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(n, _)| !is_skipped(n))
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
            walk_into(root, &path, depth + 1, out);
        } else {
            out.push(rel);
        }
    }
}

/// Everything under `planning/` and `.planner/`, paths shown relative to the
/// repo root so agents can grab exact filenames for imports (§6).
fn list_planning(repo: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for base in [repo.join("planning"), repo.join(".planner")] {
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
mod tests {
    use super::*;

    #[test]
    fn scans_shape_of_repo() {
        let tmp = std::env::temp_dir().join(format!("packet_ov_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("src/deep/deeper/deepest")).unwrap();
        fs::create_dir_all(tmp.join("node_modules/pkg")).unwrap();
        fs::write(tmp.join("README.md"), "# Hi\nWorld").unwrap();
        fs::write(tmp.join("Cargo.toml"), "[package]\n").unwrap();
        fs::write(tmp.join("src/lib.rs"), "//").unwrap();
        fs::write(tmp.join("node_modules/pkg/index.js"), "").unwrap();
        fs::create_dir_all(tmp.join("planning/imports")).unwrap();
        fs::write(tmp.join("planning/imports/doc.txt"), "ref").unwrap();

        let ov = scan(&tmp);
        assert!(ov.readme.as_deref().unwrap_or("").contains("# Hi"));
        assert!(ov.manifests.contains(&"cargo.toml".to_string()));
        assert!(ov.tree_lines.iter().any(|l| l == "src/"));
        assert!(!ov.tree_lines.iter().any(|l| l.starts_with("node_modules")));
        assert!(!ov.tree_lines.iter().any(|l| l.contains("deepest")));
        assert!(
            ov.planning_files
                .iter()
                .any(|l| l.contains("planning/imports/doc.txt (0 KB)"))
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn readme_truncation_caps_length() {
        let tmp = std::env::temp_dir().join(format!("packet_rd_{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("README.md"), "x".repeat(9000)).unwrap();
        let rd = find_readme(&tmp).unwrap();
        assert!(rd.chars().count() <= MAX_README_CHARS + 5);
        assert!(rd.ends_with("…[truncated]"));
        let _ = fs::remove_dir_all(&tmp);
    }
}
