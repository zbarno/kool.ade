//! Filesystem layout for planning artifacts inside the connected repository
//! (SPECIFICATION.md §4, §21) plus small shared IO helpers.
//!
//! Named-file module convention: this file declares submodules; each artifact
//! file has its own small neighbor.


use std::path::{Path, PathBuf};

/// Repository-relative directory holding human-facing planning documents.
pub const PLANNING_DIR: &str = "planning";
/// `planning/specification.md` — the current complete specification.
pub const SPEC_FILE: &str = "planning/specification.md";
/// `planning/open-items.md` — the serialized open-item queue.
pub const OPEN_ITEMS_FILE: &str = "planning/open-items.md";
/// `planning/imports/` — user-imported reference documents.
pub const IMPORTS_DIR: &str = "planning/imports";
/// Repository-local planner configuration directory.
pub const CONFIG_DIR: &str = ".planner";
/// `.planner/config.md` — stakeholders + current user.
pub const CONFIG_FILE: &str = ".planner/config.md";
/// `.planner/mcp.json` — MCP servers advertised to the planning session.
pub const MCP_CONFIG_FILE: &str = ".planner/mcp.json";

/// Absolute path for a repository-relative artifact.
pub fn repo_artifact(repo_root: &Path, rel: &str) -> PathBuf {
    repo_root.join(rel)
}

/// Read a UTF-8 file, reporting a clean error message when absent.
pub fn read_utf8_lossy(path: &Path) -> anyhow::Result<String> {
    std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("cannot read {}: {e}", path.display()))
}

/// Atomically write `text` to `path` (write-to-temp + rename) so a crash
/// mid-write can never truncate a planning artifact. Parent dirs are created.
pub fn atomic_write(path: &Path, text: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("packet.tmp");
    std::fs::write(&tmp, text.as_bytes())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        anyhow::anyhow!("cannot write {}: {e}", path.display())
    })
}

/// Sanitize a file name so it is safe on POSIX/Windows and never escapes
/// its directory. Returns the cleaned name (never empty).
pub fn sanitize_basename(name: &str) -> String {
    let raw = name.trim();
    // Both slash styles act as separators; drop everything before the final
    // segment (absolute/relative prefixes, ".." climbs, windows shares).
    let base = raw.split(['/', '\\']).next_back().unwrap_or("");
    let mut out: String = base
        .chars()
        .filter(|c| !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let had_leading = out.starts_with('.') || out.starts_with('-');
    while let Some(f) = out.chars().next() {
        if f == '.' || f == '-' {
            out.remove(0);
        } else {
            break;
        }
    }
    // Preserve hidden-name visibility without hiding behind a dot.
    if had_leading && !out.is_empty() {
        out.insert(0, '_');
    }
    if out.is_empty() {
        out = "import".to_string();
    }
    out.chars().take(120).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_paths_join_relatively() {
        let p = repo_artifact(Path::new("/repo"), SPEC_FILE);
        assert_eq!(p, PathBuf::from("/repo/planning/specification.md"));
    }

    #[test]
    fn sanitizer_strips_dangerous_chars_and_leading_dot() {
        assert_eq!(sanitize_basename("../evil.rs"), "evil.rs");
        assert_eq!(sanitize_basename("..\\win.dll"), "win.dll");
        assert_eq!(sanitize_basename(".hidden"), "_hidden");
        assert_eq!(sanitize_basename("a:b?.c"), "ab.c");
        assert_eq!(sanitize_basename("   "), "import");
    }
}
