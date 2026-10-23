use std::process::Command;

pub(super) fn worktree_is_clean(path: &str) -> bool {
    git_output(
        path,
        &["status", "--porcelain=v1", "--untracked-files=normal"],
    )
    .is_some_and(|status| status.is_empty())
}

pub(super) fn head_commit(path: &str) -> Option<String> {
    git_output(path, &["rev-parse", "HEAD"]).filter(|value| valid_commit(value))
}

pub(super) fn rerun_paths(path: &str) -> Vec<String> {
    ["HEAD", "index", "packed-refs", "refs/heads"]
        .into_iter()
        .filter_map(|git_path| {
            git_output(
                path,
                &[
                    "rev-parse",
                    "--path-format=absolute",
                    "--git-path",
                    git_path,
                ],
            )
        })
        .collect()
}

pub(super) fn valid_commit(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn git_output(path: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::{head_commit, rerun_paths, valid_commit, worktree_is_clean};
    use std::{path::Path, process::Command};

    /// Production boundary: verifies the Git subprocess used by `build.rs`.
    /// Pure status-string tests cannot prove the actual command, repository path,
    /// or tracked/untracked response behavior. Requires local Git and one small
    /// temporary repository; setup cost is low and no network or Docker is used.
    #[test]
    fn build_identity_is_available_only_for_a_clean_git_worktree() {
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "--quiet", "--initial-branch=main"]);
        git(
            temp.path(),
            &["config", "user.name", "Build Identity Fixture"],
        );
        git(
            temp.path(),
            &["config", "user.email", "build-identity@example.invalid"],
        );
        std::fs::write(temp.path().join("tracked.txt"), "baseline\n").unwrap();
        git(temp.path(), &["add", "tracked.txt"]);
        git(
            temp.path(),
            &["commit", "--quiet", "-m", "fixture baseline"],
        );

        assert!(worktree_is_clean(temp.path().to_str().unwrap()));
        assert!(head_commit(temp.path().to_str().unwrap()).is_some());
        let watched = rerun_paths(temp.path().to_str().unwrap());
        assert!(watched.iter().any(|path| path.ends_with("/HEAD")));
        assert!(watched.iter().any(|path| path.ends_with("/index")));
        assert!(watched.iter().any(|path| path.ends_with("/packed-refs")));
        assert!(watched.iter().any(|path| path.ends_with("/refs/heads")));

        std::fs::write(temp.path().join("tracked.txt"), "modified\n").unwrap();
        assert!(!worktree_is_clean(temp.path().to_str().unwrap()));
        std::fs::write(temp.path().join("tracked.txt"), "baseline\n").unwrap();
        assert!(worktree_is_clean(temp.path().to_str().unwrap()));

        std::fs::write(temp.path().join("untracked.txt"), "untracked\n").unwrap();
        assert!(!worktree_is_clean(temp.path().to_str().unwrap()));
    }

    #[test]
    fn commit_identity_requires_a_full_object_id() {
        assert!(valid_commit(&"a".repeat(40)));
        assert!(valid_commit(&"B".repeat(64)));
        assert!(!valid_commit("unknown"));
        assert!(!valid_commit(&"a".repeat(41)));
    }

    fn git(path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
