//! Runtime persistence that lives OUTSIDE the git repository.
//!
//! Per product decision: chat history is persistent, but it is per-operator
//! working state, not team-shared planning content — so it is stored under
//! `~/.packet/` (overridable with `$PACKET_HOME`) keyed by repository slug.
//! Git remains the sole store for *shared* planning artifacts.

use std::path::{Path, PathBuf};

/// Root of the per-user state directory. Honors `PACKET_HOME` (tests/devs).
pub fn state_root() -> PathBuf {
    if let Ok(custom) = std::env::var("PACKET_HOME") {
        if !custom.is_empty() {
            return PathBuf::from(custom);
        }
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    home.join(".packet")
}

/// Directory holding one project's runtime files.
pub fn project_dir(slug: &str) -> PathBuf {
    state_root().join("projects").join(slug)
}

/// Derive a stable, readable slug for a repository:
/// `<basename>-<16-hex-char fnv1a of canonical path>`.
pub fn project_slug(repo_canonical: &Path) -> String {
    let base = repo_canonical
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".into());
    let base: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let digest = fnv1a64(canonical_display(repo_canonical).as_bytes());
    format!("{base}-{:016x}", digest)
}

fn canonical_display(p: &Path) -> String {
    p.canonicalize()
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.to_string_lossy().into_owned())
}

/// 64-bit FNV-1a (stable across processes/platforms — no dependency needed).
pub fn fnv1a64(data: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// Record mapping slug → repository path so the UI can recognize projects
/// it has seen before (written opportunistically on connect).
pub fn known_projects_path() -> PathBuf {
    state_root().join("projects.index.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_is_stable_readable_and_path_sensitive() {
        let p = Path::new("/home/u/work/demo-app");
        let a = project_slug(p);
        let b = project_slug(Path::new("/home/u/work/demo-app/."));
        assert_ne!(a, b, "different canonical paths must differ");
        assert!(a.starts_with("demo-app-"));
        assert_eq!(a.len(), "demo-app-".len() + 16);
        // Same spelling → same digest portion (without canonicalize they match;
        // canonicalize makes them equal on the same machine too).
        assert_eq!(project_slug(p), a);
    }

    #[test]
    fn fnv_vector_known_value() {
        // FNV-1a 64 of "" and "a" (published constants).
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
