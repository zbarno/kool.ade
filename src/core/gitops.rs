//! Minimal git behavior (SPECIFICATION.md §20): read status, stage exactly
//! the planning paths we touched, and commit planner checkpoints.
//!
//! We shell out to the system `git` rather than embedding libgit2: the CLI
//! is the canonical collaborator's surface, and the dependency footprint
//! stays tiny. All arguments pass through `Command` arrays (no shell
//! interpolation), paths always after `--`.

use std::path::Path;
use std::process::Command;

use crate::error::AppError;

pub const AUTHOR_NAME: &str = "Packet Planner";
pub const AUTHOR_EMAIL: &str = "planner@packet.local";

/// One-shot git snapshot for the header/UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitSnapshot {
    pub branch: String,
    pub head_short: String,
    /// Number of dirty entries (modified/untracked) in the work tree.
    pub dirty: usize,
    pub last_subject: String,
}

fn run(cwd: &Path, args: &[&str]) -> Result<(i32, String, String), AppError> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|e| AppError::Git {
            cmd: format!("git {}", args.join(" ")),
            detail: format!("git binary could not be launched: {e}"),
        })?;
    Ok((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    ))
}

/// True when `cwd` is inside a git working tree.
pub fn is_work_tree(cwd: &Path) -> bool {
    if !cwd.is_dir() {
        return false;
    }
    let Ok((code, _, _)) = run(cwd, &["rev-parse", "--is-inside-work-tree"]) else {
        return false;
    };
    code == 0
}

/// True when `relative` is known to the repository rooted at `cwd`
/// (`git ls-files --error-unmatch`, dispatched through [`run`] so the
/// NFR-5 vector-argument/no-shell guarantee stays centralized). Probe
/// failure counts as `false` (caller treats the path as unstaged), which
/// errs toward NOT checkpointing a path the index cannot attest.
pub fn tracked_in_head(cwd: &Path, relative: &str) -> bool {
    match run(cwd, &["ls-files", "--error-unmatch", relative]) {
        Ok((code, _, _)) => code == 0,
        Err(_) => false,
    }
}

/// Clone `source` (a remote URL or a local path) into `dest` via the
/// system `git` CLI.
///
/// Dispatched through [`run`] so the NFR-5 vector-argument/no-shell
/// guarantee stays centralized; `cwd` is `dest.parent()`, which callers
/// guarantee exists (git refuses to create the parent itself). A nonzero
/// exit surfaces as [`AppError::Git`] carrying git's trimmed stderr (or
/// stdout when stderr is blank).
pub fn clone_repo(source: &str, dest: &Path) -> Result<(), AppError> {
    let parent = dest.parent().ok_or_else(|| AppError::Io {
        op: "prepare clone destination".into(),
        detail: "the clone destination has no parent directory".into(),
    })?;
    let dest_arg = dest.to_string_lossy().into_owned();
    let (code, out, err) = run(parent, &["clone", source, &dest_arg])?;
    if code != 0 {
        let detail = err.trim();
        let detail = if detail.is_empty() {
            out.trim()
        } else {
            detail
        };
        return Err(AppError::Git {
            cmd: format!("clone {source}"),
            detail: detail.to_string(),
        });
    }
    Ok(())
}

/// Gather branch/head/dirty/last-subject. Individual probe failures degrade
/// gracefully to blanks rather than breaking the UI.
pub fn snapshot(cwd: &Path) -> GitSnapshot {
    let mut snap = GitSnapshot::default();
    if let Ok((_, out, _)) = run(cwd, &["symbolic-ref", "--short", "HEAD"]) {
        snap.branch = out.trim().to_string();
    }
    if snap.branch.is_empty() {
        // Detached HEAD: label by short sha.
        if let Ok((code, out, _)) = run(cwd, &["rev-parse", "--short", "HEAD"]) {
            if code == 0 {
                snap.branch = format!("detached @{head}", head = out.trim());
            }
        }
    }
    if let Ok((code, out, _)) = run(cwd, &["rev-parse", "--short", "HEAD"]) {
        if code == 0 {
            snap.head_short = out.trim().to_string();
        }
    }
    if let Ok((code, out, _)) = run(cwd, &["status", "--porcelain=v1"]) {
        if code == 0 {
            snap.dirty = out.lines().count();
        }
    }
    if let Ok((code, out, _)) = run(cwd, &["log", "-1", "--pretty=%s"]) {
        if code == 0 {
            snap.last_subject = out.trim().to_string();
        }
    }
    snap
}

/// Read a single git config value using standard nearest-scope-wins
/// resolution (repository-local, then global, then system — no explicit
/// `--local/--global/--system` flag). Read-only, argument-array dispatched
/// per NFR-5.
///
/// Failure is TERMINAL BY DESIGN into `None`, never into `Err`: a missing
/// binary, a non-repo directory, an unset key, or a whitespace-only value
/// all collapse to `None`, letting the FR-13 identity tier ladder
/// (`user.name` → `user.email` → config block → `(guest)`) simply descend.
/// An identity probe must never block a connect (approved spec §9 soft
/// precondition: degraded but functional, never blocked).
///
/// A directory that is NOT inside a git working tree resolves to `None`
/// without consulting the ambient layers: outside a repository, plain
/// `git config <key>` falls back to the operator's global/system hierarchy,
/// which would leak an identity the connected tree never declared.
pub fn read_config(cwd: &Path, key: &str) -> Option<String> {
    if !is_work_tree(cwd) {
        return None;
    }
    let (code, out, _) = run(cwd, &["config", key]).ok()?;
    if code != 0 {
        return None;
    }
    let value = out.trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Stage exactly the given repo-relative paths.
pub fn stage(cwd: &Path, paths: &[String]) -> Result<(), AppError> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args: Vec<String> = vec!["add".into(), "-A".into(), "--".into()];
    args.extend(paths.iter().cloned());
    let as_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (_, _, err) = run(cwd, &as_refs)?;
    if !err.trim().is_empty() {
        return Err(AppError::Git {
            cmd: "add".into(),
            detail: err,
        });
    }
    Ok(())
}

/// Commit the staged planning paths with a planner-authored identity.
/// Returns the short SHA. Fails (with git's stderr) when the repo has no
/// commits-policy issue like an empty index — caller decides messaging.
pub fn commit(cwd: &Path, message: &str, paths: &[String]) -> Result<String, AppError> {
    stage(cwd, paths)?;
    let args = [
        "-c",
        &format!("user.name={AUTHOR_NAME}"),
        "-c",
        &format!("user.email={AUTHOR_EMAIL}"),
        "commit",
        "-q",
        "-m",
        message,
    ];
    let as_refs: Vec<&str> = args.iter().copied().collect();
    let (code, out, err) = run(cwd, &as_refs)?;
    let combined = format!("{out}{err}");
    if code != 0 && !combined.to_ascii_lowercase().contains("nothing to commit") {
        return Err(AppError::Git {
            cmd: "commit".into(),
            detail: combined.trim().to_string(),
        });
    }
    let (_, sha, _) = run(cwd, &["rev-parse", "--short", "HEAD"])?;
    Ok(sha.trim().to_string())
}

/// Convenience: commit only the canonical planning artifacts.
pub fn commit_planning_changes(
    cwd: &Path,
    message: &str,
    paths: &[String],
) -> Result<String, AppError> {
    commit(cwd, message, paths)
}

/// Test-only support for detaching git's AMBIENT (global/system) identity
/// hierarchy. Production code never touches these variables: `read_config`
/// deliberately uses the standard hierarchy (nearest scope wins) per D-23.
///
/// Precedent for process-global env mutation under a lock: `ENV_HOME_LOCK`
/// in `crate::persistence::chat_store::tests`.
#[cfg(test)]
pub(crate) mod test_support {
    use std::env;
    use std::fs;
    use std::sync::{Mutex, MutexGuard};

    /// Held for the ENTIRE body of any shielded test so sibling git spawns
    /// observe a consistent hierarchy, and so our mutation cannot bleed into
    /// any sibling. (Local repo configs are unaffected by the shield; only
    /// the global/system layers are swapped for empty control files.)
    static GIT_HIERARCHY_LOCK: Mutex<()> = Mutex::new(());

    /// Point `GIT_CONFIG_GLOBAL` / `GIT_CONFIG_SYSTEM` (honored by git ≥
    /// 2.32) at empty control files; releases the env vars and removes the
    /// control dir on drop.
    pub fn shield(tag: &str) -> Guard {
        let lock = GIT_HIERARCHY_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let dir = env::temp_dir().join(format!("packet_git_shield_{tag}_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let global = dir.join("control.global.empty");
        let system = dir.join("control.system.empty");
        fs::write(&global, "").unwrap();
        fs::write(&system, "").unwrap();
        // SAFETY: guarded by GIT_HIERARCHY_LOCK; no other test mutates or
        // depends on the ambient git hierarchy while the guard is held.
        unsafe {
            env::set_var("GIT_CONFIG_GLOBAL", &global);
            env::set_var("GIT_CONFIG_SYSTEM", &system);
        }
        Guard {
            _lock: lock,
            _cleanup: dir,
        }
    }

    pub(crate) struct Guard {
        _lock: MutexGuard<'static, ()>,
        // Pid/tag-scoped control dir, removed on drop so repeated suite runs
        // leave no litter in the temp dir (still held under the lock).
        _cleanup: std::path::PathBuf,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: same lock-guarded invariant as set in `shield`.
            unsafe {
                env::remove_var("GIT_CONFIG_GLOBAL");
                env::remove_var("GIT_CONFIG_SYSTEM");
            }
            let _ = fs::remove_dir_all(&self._cleanup);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn fresh_dir(prefix: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("packet_git_{prefix}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn git_in(p: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(p)
            .args(args)
            .output()
            .unwrap()
            .stdout;
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Temp repo with a committed file and a LOCAL planner-style identity.
    fn mkrepo(prefix: &str) -> PathBuf {
        let p = fresh_dir(prefix);
        let _ = git_in(&p, &["init", "-q", "-b", "main"]);
        let _ = git_in(&p, &["config", "user.name", "T"]);
        let _ = git_in(&p, &["config", "user.email", "t@x"]);
        fs::write(p.join("a.txt"), "one").unwrap();
        let _ = git_in(&p, &["add", "a.txt"]);
        let _ = git_in(&p, &["commit", "-qm", "init"]);
        p
    }

    /// Fresh empty directory (deliberately NOT a git repository).
    fn plain_dir(prefix: &str) -> PathBuf {
        fresh_dir(prefix)
    }

    #[test]
    fn snapshot_reports_branch_dirty_and_subject() {
        let repo = mkrepo("snap");
        let snap = snapshot(&repo);
        assert_eq!(snap.branch, "main");
        assert!(!snap.head_short.is_empty());
        assert_eq!(snap.last_subject, "init");
        assert_eq!(snap.dirty, 0);
        fs::write(repo.join("b.txt"), "two").unwrap();
        let snap2 = snapshot(&repo);
        assert_eq!(snap2.dirty, 1);
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn commit_lands_a_checkpoint_with_planner_authorship() {
        let repo = mkrepo("commit");
        fs::write(repo.join("planning.md"), "spec").unwrap();
        let sha = commit(
            &repo,
            "planner: establish initial specification",
            &["planning.md".into()],
        )
        .unwrap();
        let log = git_in(&repo, &["log", "-1", "--pretty=%an %s"]);
        assert!(log.contains(AUTHOR_NAME));
        assert!(log.contains("planner: establish initial specification"));
        assert!(!sha.is_empty());
        let _ = fs::remove_dir_all(&repo);
    }

    // ---- clone_repo -------------------------------------------------------

    #[test]
    fn clone_repo_clones_local_source_into_dest() {
        let source = mkrepo("clonesrc");
        let dest = source.with_file_name(format!(
            "{}_cloned",
            source
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("repo")
        ));
        clone_repo(source.to_str().unwrap(), &dest).unwrap();
        assert!(is_work_tree(&dest), "the clone is a working tree");
        assert_eq!(
            fs::read_to_string(dest.join("a.txt")).unwrap(),
            "one",
            "committed files arrived"
        );
        let _ = fs::remove_dir_all(&dest);
        let _ = fs::remove_dir_all(&source);
    }

    #[test]
    fn clone_repo_missing_source_errors_git() {
        let dir = fresh_dir("clonesrcmiss");
        let missing = dir.join("never-init-as-repo");
        let dest = dir.join("cloned");
        match clone_repo(missing.to_str().unwrap(), &dest) {
            Err(AppError::Git { cmd, detail }) => {
                assert!(cmd.starts_with("clone "), "cmd labels the clone: {cmd}");
                assert!(!detail.trim().is_empty(), "git's stderr tail rides along");
            }
            other => panic!("expected Git error for a nonexistent source, got {other:?}"),
        }
        assert!(
            !dest.exists(),
            "no half-made destination for a failed clone"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clone_repo_missing_dest_parent_errors() {
        let dir = fresh_dir("clonparent");
        let source = mkrepo("clonparent-src");
        let dest = dir.join("does-not-exist").join("deeper").join("clone");
        assert!(clone_repo(source.to_str().unwrap(), &dest).is_err());
        let _ = fs::remove_dir_all(&source);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_repo_detection() {
        let p = plain_dir("norepo");
        assert!(!is_work_tree(&p));
        let _ = fs::remove_dir_all(&p);
    }

    #[test]
    fn read_config_returns_local_values_via_standard_resolution() {
        let repo = mkrepo("cfg");
        assert_eq!(read_config(&repo, "user.name"), Some("T".into()));
        assert_eq!(read_config(&repo, "user.email"), Some("t@x".into()));
        // Nobody sets such a key: absent → None, not an error.
        assert_eq!(read_config(&repo, "packet.probe.no.such.key"), None);
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn read_config_reads_email_only_under_shielded_ambient() {
        // The ambient machine may carry a GLOBAL identity; point the global/
        // system layers at empty control files so only the REPO-LOCAL value
        // can influence resolution.
        let _shield = super::test_support::shield("cfg-email");
        let p = fresh_dir("cfe");
        let _ = git_in(&p, &["init", "-q", "-b", "main"]);
        let _ = git_in(&p, &["config", "user.email", "eve@example.org"]);
        // user.name deliberately never set locally or ambient.
        assert_eq!(read_config(&p, "user.name"), None);
        assert_eq!(
            read_config(&p, "user.email"),
            Some("eve@example.org".into())
        );
        let _ = fs::remove_dir_all(&p);
    }

    #[test]
    fn read_config_swallows_non_repo_and_blank_value_failures() {
        // Plain non-git temp dir: probe must degrade to None, never panic.
        let p = plain_dir("cfgplain");
        assert_eq!(read_config(&p, "user.name"), None);
        assert_eq!(read_config(&p, "user.email"), None);
        let _ = fs::remove_dir_all(&p);

        // Blank (whitespace-only) local value: trimmed to absent.
        let _shield = super::test_support::shield("cfgblank");
        let r = fresh_dir("cfgb");
        let _ = git_in(&r, &["init", "-q", "-b", "main"]);
        let _ = git_in(&r, &["config", "user.name", "   "]);
        assert_eq!(read_config(&r, "user.name"), None);
        let _ = fs::remove_dir_all(&r);
    }

    #[test]
    fn non_existent_cwd_degrades_to_none() {
        assert_eq!(
            read_config(Path::new("/no/such/cwd-xyz"), "user.name"),
            None
        );
    }
}
