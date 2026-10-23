use crate::core::self_improvement::SelfImprovementParentTaskProgressV1;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

const ANCESTRY_RETRY_DELAY: Duration = Duration::from_secs(30);
const ANCESTRY_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_ANCESTRY_CACHE_ENTRIES: usize = 64;

#[cfg(test)]
pub(super) fn verify_parent_merge_ancestry(
    canonical_remote: &str,
    progress: &[SelfImprovementParentTaskProgressV1],
    latest_sha: &str,
    gh_bin: &Path,
) -> bool {
    check_parent_merge_ancestry(canonical_remote, progress, latest_sha, gh_bin).is_ok()
}

fn check_parent_merge_ancestry(
    canonical_remote: &str,
    progress: &[SelfImprovementParentTaskProgressV1],
    latest_sha: &str,
    gh_bin: &Path,
) -> Result<(), String> {
    let repository = canonical_remote
        .strip_prefix("https://github.com/")
        .and_then(|value| value.strip_suffix(".git").or(Some(value)))
        .ok_or_else(|| "merge ancestry requires a canonical GitHub HTTPS remote".to_owned())?;
    let mut repository_parts = repository.split('/');
    if repository_parts.next().is_none_or(str::is_empty)
        || repository_parts.next().is_none_or(str::is_empty)
        || repository_parts.next().is_some()
        || !repository
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
    {
        return Err("merge ancestry remote is malformed".into());
    }
    for earlier in progress
        .iter()
        .filter_map(|item| item.pr_merged_commit_sha.as_deref())
        .filter(|sha| *sha != latest_sha)
    {
        let comparison = format!("repos/{repository}/compare/{earlier}...{latest_sha}");
        let output = ancestry_compare(gh_bin, &comparison)?;
        if !output.status.success() {
            return Err("GitHub ancestry query failed".into());
        }
        if !matches!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "ahead" | "identical"
        ) {
            return Err("GitHub did not confirm that parent merge commits are ancestors".into());
        }
    }
    if !progress
        .iter()
        .all(|item| item.pr_merged_commit_sha.is_some())
    {
        return Err("merge ancestry evidence is incomplete".into());
    }
    Ok(())
}

pub(super) fn parent_merge_ancestry_verified(
    activity: &mut crate::app::manager::WorkspaceActivity,
    canonical_remote: &str,
    progress: &[SelfImprovementParentTaskProgressV1],
    latest_sha: &str,
    gh_bin: &Path,
) -> Result<bool, String> {
    let key = ancestry_key(canonical_remote, progress, latest_sha)?;
    if activity.verified_parent_merge_ancestry.contains(&key) {
        return Ok(true);
    }
    if let Some((_, retry_after)) = activity.failed_parent_merge_ancestry.get(&key)
        && Instant::now() < *retry_after
    {
        return Ok(false);
    }

    if activity.parent_merge_ancestry_check.is_some() {
        let (pending_key, result) = {
            let (pending_key, receiver) = activity.parent_merge_ancestry_check.as_ref().unwrap();
            (pending_key.clone(), receiver.try_recv())
        };
        match result {
            Ok(result) => {
                activity.parent_merge_ancestry_check = None;
                match result {
                    Ok(()) => {
                        remember_verified_ancestry(activity, pending_key.clone());
                        if pending_key == key {
                            return Ok(true);
                        }
                    }
                    Err(error) => {
                        remember_failed_ancestry(activity, pending_key.clone(), error.clone());
                        if pending_key == key {
                            return Err(error);
                        }
                    }
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(false),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                activity.parent_merge_ancestry_check = None;
                return Err("merge ancestry provider check stopped unexpectedly".into());
            }
        }
    }

    let remote = canonical_remote.to_owned();
    let parents = progress.to_vec();
    let latest = latest_sha.to_owned();
    let gh_bin = gh_bin.to_owned();
    let (sender, receiver) = std::sync::mpsc::channel();
    let _worker = std::thread::Builder::new()
        .name("self-improvement-ancestry".into())
        .spawn(move || {
            let result = check_parent_merge_ancestry(&remote, &parents, &latest, &gh_bin);
            let _ = sender.send(result);
        })
        .map_err(|error| format!("could not start merge ancestry worker: {error}"))?;
    activity.parent_merge_ancestry_check = Some((key, receiver));
    Ok(false)
}

fn remember_verified_ancestry(activity: &mut crate::app::manager::WorkspaceActivity, key: String) {
    if activity.verified_parent_merge_ancestry.len() >= MAX_ANCESTRY_CACHE_ENTRIES {
        activity.verified_parent_merge_ancestry.clear();
    }
    activity.verified_parent_merge_ancestry.insert(key.clone());
    activity.failed_parent_merge_ancestry.remove(&key);
}

fn remember_failed_ancestry(
    activity: &mut crate::app::manager::WorkspaceActivity,
    key: String,
    error: String,
) {
    if activity.failed_parent_merge_ancestry.len() >= MAX_ANCESTRY_CACHE_ENTRIES {
        activity.failed_parent_merge_ancestry.clear();
    }
    activity
        .failed_parent_merge_ancestry
        .insert(key, (error, Instant::now() + ANCESTRY_RETRY_DELAY));
}

fn ancestry_compare(gh_bin: &Path, comparison: &str) -> Result<std::process::Output, String> {
    ancestry_compare_with_timeout(gh_bin, comparison, ANCESTRY_REQUEST_TIMEOUT)
}

pub(super) fn ancestry_compare_with_timeout(
    gh_bin: &Path,
    comparison: &str,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    let mut child = Command::new(gh_bin)
        .args([
            "api",
            comparison,
            "--jq",
            ".status",
            "--hostname",
            "github.com",
        ])
        .env("GH_PROMPT_DISABLED", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("could not start GitHub ancestry query: {error}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("could not poll GitHub ancestry query: {error}"));
            }
        };
        if status.is_some() {
            return child
                .wait_with_output()
                .map_err(|error| format!("could not collect GitHub ancestry query: {error}"));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "GitHub ancestry query exceeded its {}-second timeout",
                timeout.as_secs_f32()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn ancestry_key(
    canonical_remote: &str,
    progress: &[SelfImprovementParentTaskProgressV1],
    latest_sha: &str,
) -> Result<String, String> {
    if canonical_remote
        .strip_prefix("https://github.com/")
        .is_none()
    {
        return Err("merge ancestry requires a canonical GitHub HTTPS remote".into());
    }
    if progress.is_empty()
        || progress
            .iter()
            .any(|item| item.pr_merged_commit_sha.is_none())
    {
        return Err("merge ancestry evidence is incomplete".into());
    }
    let mut shas = progress
        .iter()
        .filter_map(|item| item.pr_merged_commit_sha.as_deref())
        .collect::<Vec<_>>();
    shas.sort_unstable();
    Ok(format!(
        "{canonical_remote}:{latest_sha}:{}",
        shas.join(",")
    ))
}
