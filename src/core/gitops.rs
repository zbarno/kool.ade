//! Minimal git behavior (SPECIFICATION.md §20): read status, stage exactly
//! the planning paths we touched, and commit planner checkpoints.
//!
//! We shell out to the system `git` rather than embedding libgit2: the CLI
//! is the canonical collaborator's surface, and the dependency footprint
//! stays tiny. All arguments pass through `Command` arrays (no shell
//! interpolation), paths always after `--`.

use std::io::Write;
use std::path::Path;
use std::process::Stdio;

use crate::error::AppError;

mod commit;
mod snapshot;
#[cfg(test)]
pub(crate) use commit::commit_planning_changes_at;
pub(crate) use commit::lock_repository;
pub use commit::{commit, commit_cancellable, commit_planning_changes};
pub use snapshot::snapshot;

pub const AUTHOR_NAME: &str = "Kool.ad/e Planner";
pub const AUTHOR_EMAIL: &str = "planner@koolade.local";

/// One-shot git snapshot for the header/UI.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitSnapshot {
    pub branch: String,
    pub branches: Vec<String>,
    pub remote_branches: Vec<String>,
    pub has_origin: bool,
    pub default_branch: String,
    pub head_short: String,
    /// Number of dirty entries (modified/untracked) in the work tree.
    pub dirty: usize,
    pub last_subject: String,
}

fn run(cwd: &Path, args: &[&str]) -> Result<(i32, String, String), AppError> {
    let mut guarded_args = args.to_vec();
    if guarded_args.first() == Some(&"diff") {
        guarded_args.splice(1..1, ["--no-ext-diff", "--no-textconv"]);
    }
    let out = crate::core::planning_sync::validated_git_command(cwd)
        .map_err(|error| AppError::Git {
            cmd: crate::error::redact_secrets(&format!("git {}", guarded_args.join(" "))),
            detail: crate::error::redact_secrets(&error.to_string()),
        })?
        .args(&guarded_args)
        .output()
        .map_err(|e| AppError::Git {
            cmd: crate::error::redact_secrets(&format!("git {}", guarded_args.join(" "))),
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
    let mut child = crate::core::planning_sync::validated_git_command(cwd)
        .map_err(|error| AppError::Git {
            cmd: crate::error::redact_secrets(&format!("git {}", args.join(" "))),
            detail: crate::error::redact_secrets(&error.to_string()),
        })?
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

/// Return the configured `origin` URL when the repository has one. Callers
/// use this to verify that a managed checkout still belongs to the remote
/// requested by the operator before reusing it.
pub fn origin_url(cwd: &Path) -> Option<String> {
    run(cwd, &["remote", "get-url", "origin"])
        .ok()
        .filter(|(code, _, _)| *code == 0)
        .map(|(_, stdout, _)| stdout.trim().to_owned())
        .filter(|remote| !remote.is_empty())
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
    let (code, out, err) = run(
        parent,
        &[
            "-c",
            "user.name=Kool.ad/e repository initialization",
            "-c",
            "user.email=repository-initialization@koolade.invalid",
            "clone",
            source,
            &dest_arg,
        ],
    )?;
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
    // Identity is intentionally resolved using the operator's standard Git
    // config hierarchy. This read-only query cannot invoke hooks or helpers;
    // preserve GIT_CONFIG_GLOBAL/SYSTEM so callers can isolate that hierarchy.
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["config", key])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout);
    let value = value.trim();
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
pub(crate) mod test_support;

#[cfg(test)]
mod tests;
