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
    let Ok((code, _, _)) = run(cwd, &["rev-parse", "--is-inside-work-tree"])
    else {
        return false;
    };
    code == 0
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
pub fn commit_planning_changes(cwd: &Path, message: &str, paths: &[String]) -> Result<String, AppError> {
    commit(cwd, message, paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn mkrepo(prefix: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("packet_git_{prefix}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        let g = |args: &[&str]| -> String {
            let out = Command::new("git")
                .arg("-C")
                .arg(&p)
                .args(args)
                .output()
                .unwrap()
                .stdout;
            String::from_utf8_lossy(&out).into_owned()
        };
        let _ = g(&["init", "-q", "-b", "main"]);
        let _ = g(&["config", "user.name", "T"]);
        let _ = g(&["config", "user.email", "t@x"]);
        fs::write(p.join("a.txt"), "one").unwrap();
        let _ = g(&["add", "a.txt"]);
        let _ = g(&["commit", "-qm", "init"]);
        p
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
        let sha = commit(&repo, "planner: establish initial specification", &["planning.md".into()]).unwrap();
        let log = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(["log", "-1", "--pretty=%an %s"])
            .output()
            .unwrap();
        let line = String::from_utf8(log.stdout).unwrap();
        assert!(line.contains(AUTHOR_NAME));
        assert!(line.contains("planner: establish initial specification"));
        assert!(!sha.is_empty());
        let _ = fs::remove_dir_all(&repo);
    }

    #[test]
    fn non_repo_detection() {
        let p = std::env::temp_dir().join(format!("packet_norepo_{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        assert!(!is_work_tree(&p));
        let _ = fs::remove_dir_all(&p);
    }
}
