use std::{env, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=KOOLADE_BUILD_COMMIT");
    let configured = env::var("KOOLADE_BUILD_COMMIT").ok();
    let clean = worktree_is_clean();
    let detected = clean.then(|| git_output(&["rev-parse", "HEAD"])).flatten();
    let commit = detected
        .filter(|value| valid_commit(value))
        .or_else(|| {
            clean
                .then(|| configured.filter(|value| valid_commit(value)))
                .flatten()
        })
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=KOOLADE_BUILD_COMMIT={commit}");
}

fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn worktree_is_clean() -> bool {
    git_output(&["status", "--porcelain=v1", "--untracked-files=normal"])
        .is_some_and(|status| status_is_clean(&status))
}

fn valid_commit(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::valid_commit;

    #[test]
    fn commit_identity_requires_a_full_object_id() {
        assert!(valid_commit(&"a".repeat(40)));
        assert!(valid_commit(&"B".repeat(64)));
        assert!(!valid_commit("unknown"));
        assert!(!valid_commit(&"a".repeat(41)));
    }

    #[test]
    fn worktree_status_requires_no_tracked_or_untracked_changes() {
        assert!(super::status_is_clean(""));
        assert!(!super::status_is_clean(" M src/lib.rs\n"));
        assert!(!super::status_is_clean("?? src/new_feature.rs\n"));
    }
}

fn status_is_clean(status: &str) -> bool {
    status.is_empty()
}
