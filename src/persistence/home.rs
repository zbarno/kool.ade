//! Private application data that lives outside source repositories.
//!
//! Task trees, conversations, decisions, approvals, and runtime evidence are
//! stored under `~/.koolade-packet/` (overridable with `$KOOLADE_HOME`) and
//! keyed by the primary code repository identity.

use std::path::{Path, PathBuf};

/// Root of the per-user state directory. Honors `KOOLADE_HOME` (tests/devs).
pub fn state_root() -> PathBuf {
    if let Ok(custom) = std::env::var("KOOLADE_HOME")
        && !custom.is_empty()
    {
        return PathBuf::from(custom);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    home.join(".koolade-packet")
}

/// Directory holding one project's runtime files.
pub fn project_dir(slug: &str) -> PathBuf {
    state_root().join("projects").join(slug)
}

/// Derive a stable, readable per-installation key for a code repository.
/// Repositories with an origin use that identity so moving a checkout does
/// not strand local task history; repositories without an origin use their
/// canonical path until an origin is configured.
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
    let identity = repository_identity(repo_canonical);
    let digest = fnv1a64(identity.as_bytes());
    format!("{base}-{:016x}", digest)
}

fn repository_identity(repo: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["config", "--local", "--get", "remote.origin.url"])
        .current_dir(repo)
        .output();
    if let Ok(output) = output
        && output.status.success()
        && let Ok(remote) = String::from_utf8(output.stdout)
    {
        let remote = remote.trim();
        if !remote.is_empty() {
            // Only the digest enters the local store path. A remote URL may
            // contain a credential and must never be copied into metadata.
            return format!("origin:{remote}");
        }
    }
    format!("path:{}", canonical_display(repo))
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

/// Return true when a path resolves inside any Git worktree. For paths not
/// created yet, resolve the nearest existing ancestor before checking so a
/// configured future state directory cannot escape this boundary by using a
/// symlinked parent.
pub(crate) fn is_inside_git_worktree(path: &Path) -> bool {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else if let Ok(current) = std::env::current_dir() {
        current.join(path)
    } else {
        return false;
    };
    let mut existing = absolute.as_path();
    let mut missing = Vec::new();
    while std::fs::symlink_metadata(existing).is_err() {
        let Some(name) = existing.file_name() else {
            return false;
        };
        missing.push(name.to_os_string());
        let Some(parent) = existing.parent() else {
            return false;
        };
        existing = parent;
    }
    let Ok(mut resolved) = existing.canonicalize() else {
        return false;
    };
    for component in missing.iter().rev() {
        resolved.push(component);
    }
    resolved
        .ancestors()
        .any(|ancestor| std::fs::symlink_metadata(ancestor.join(".git")).is_ok())
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
    fn repositories_with_the_same_origin_keep_their_local_identity_after_move() {
        let root = std::env::temp_dir().join(format!("koolade-origin-{}", uuid::Uuid::new_v4()));
        let first = root.join("first");
        let second = root.join("other-parent").join("first");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        for path in [&first, &second] {
            assert!(
                std::process::Command::new("git")
                    .args(["init", "-q"])
                    .current_dir(path)
                    .status()
                    .unwrap()
                    .success()
            );
            assert!(
                std::process::Command::new("git")
                    .args([
                        "config",
                        "--local",
                        "remote.origin.url",
                        "https://example.invalid/team/project.git"
                    ])
                    .current_dir(path)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        assert_eq!(project_slug(&first), project_slug(&second));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fnv_vector_known_value() {
        // FNV-1a 64 of "" and "a" (published constants).
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn git_worktree_guard_handles_future_paths_and_symlinked_parents() {
        let repository = tempfile::tempdir().unwrap();
        std::fs::create_dir(repository.path().join(".git")).unwrap();
        let future_state = repository.path().join("future/state");
        assert!(is_inside_git_worktree(&future_state));

        let external = tempfile::tempdir().unwrap();
        assert!(!is_inside_git_worktree(
            &external.path().join("future/state")
        ));
        #[cfg(unix)]
        {
            let link = external.path().join("repo-link");
            std::os::unix::fs::symlink(repository.path(), &link).unwrap();
            assert!(is_inside_git_worktree(&link.join("private-state")));
        }
    }
}
