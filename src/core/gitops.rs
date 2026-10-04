//! Minimal git behavior (SPECIFICATION.md §20): read status, stage exactly
//! the planning paths we touched, and commit planner checkpoints.
//!
//! We shell out to the system `git` rather than embedding libgit2: the CLI
//! is the canonical collaborator's surface, and the dependency footprint
//! stays tiny. All arguments pass through `Command` arrays (no shell
//! interpolation), paths always after `--`.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::AppError;

mod commit;
pub use commit::{commit, commit_cancellable, commit_planning_changes};

pub const AUTHOR_NAME: &str = "Kool.ad/e Planner";
pub const AUTHOR_EMAIL: &str = "planner@koolade.local";

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
            cmd: crate::error::redact_secrets(&format!("git {}", args.join(" "))),
            detail: format!("git binary could not be launched: {e}"),
        })?;
    Ok((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    ))
}

pub(super) fn run_with_input(
    cwd: &Path,
    args: &[&str],
    input: &[u8],
) -> Result<(i32, String, String), AppError> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Git {
            cmd: crate::error::redact_secrets(&format!("git {}", args.join(" "))),
            detail: format!("git binary could not be launched: {e}"),
        })?;
    let write_result = child
        .stdin
        .take()
        .expect("piped git stdin")
        .write_all(input);
    let out = child.wait_with_output().map_err(|e| AppError::Git {
        cmd: crate::error::redact_secrets(&format!("git {}", args.join(" "))),
        detail: format!("git process failed while collecting output: {e}"),
    })?;
    if let Err(error) = write_result {
        return Err(AppError::Git {
            cmd: crate::error::redact_secrets(&format!("git {}", args.join(" "))),
            detail: format!("could not send index data to git: {error}"),
        });
    }
    Ok((
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    ))
}

fn require_exit_success(
    command: &str,
    status: i32,
    stdout: String,
    stderr: String,
) -> Result<(), AppError> {
    if status == 0 {
        return Ok(());
    }
    let detail = if stderr.trim().is_empty() {
        stdout
    } else {
        stderr
    };
    Err(AppError::Git {
        cmd: crate::error::redact_secrets(command),
        detail: crate::error::redact_secrets(detail.trim()),
    })
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
            cmd: crate::error::redact_secrets(&format!("clone {source}")),
            detail: crate::error::redact_secrets(detail),
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
        if let Ok((code, out, _)) = run(cwd, &["rev-parse", "--short", "HEAD"])
            && code == 0
        {
            snap.branch = format!("detached @{head}", head = out.trim());
        }
    }
    if let Ok((code, out, _)) = run(cwd, &["rev-parse", "--short", "HEAD"])
        && code == 0
    {
        snap.head_short = out.trim().to_string();
    }
    if let Ok((code, out, _)) = run(cwd, &["status", "--porcelain=v1"])
        && code == 0
    {
        snap.dirty = out.lines().count();
    }
    if let Ok((code, out, _)) = run(cwd, &["log", "-1", "--pretty=%s"])
        && code == 0
    {
        snap.last_subject = out.trim().to_string();
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
    args.extend(paths.iter().map(|path| format!(":(top,literal){path}")));
    let as_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, out, err) = run(cwd, &as_refs)?;
    require_exit_success("add", code, out, err)
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
        let dir = env::temp_dir().join(format!("koolade_git_shield_{tag}_{}", std::process::id()));
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
mod tests;
