use super::super::{Implementation, Runner, state_paths::common};
use super::{Plan, support};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

const CACHE_SCHEMA: u8 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VerifiedBase {
    pub(super) schema_version: u8,
    pub(super) base: String,
    pub(super) local_commit: String,
    pub(super) remote_commit: String,
    pub(super) common_base: String,
    pub(super) verified_commit: String,
    pub(super) verification: Vec<String>,
}

pub(super) fn acquire_lock(repo: &Path, plan: &Plan, runner: &Runner) -> anyhow::Result<File> {
    let directory = cache_directory(repo)?;
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{}.lock", cache_key(plan)));
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    let mut waiting = false;
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(TryLockError::WouldBlock) => {
                runner.remaining()?;
                if !waiting {
                    runner
                        .update("Waiting for another task to verify this shared branch baseline…");
                    waiting = true;
                }
                thread::sleep(Duration::from_millis(100));
            }
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

pub(super) fn load(
    repo: &Path,
    plan: &Plan,
    runner: &Runner,
) -> anyhow::Result<Option<VerifiedBase>> {
    let key = cache_key(plan);
    let path = cache_directory(repo)?.join(format!("{key}.json"));
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    let cached: VerifiedBase = match serde_json::from_slice(&bytes) {
        Ok(cached) => cached,
        Err(_) => return Ok(None),
    };
    if cached.schema_version != CACHE_SCHEMA
        || cached.base != plan.base
        || cached.local_commit != plan.local_commit
        || cached.remote_commit != plan.remote_commit
        || cached.common_base != plan.common_base
        || plan
            .required_verification
            .iter()
            .any(|required| !cached.verification.contains(required))
    {
        return Ok(None);
    }
    let reference = cache_ref(&key);
    let pinned = match runner.git(repo, &["rev-parse", "--verify", &reference]) {
        Ok(commit) => commit,
        Err(_) => {
            runner.remaining()?;
            return Ok(None);
        }
    };
    if pinned != cached.verified_commit {
        return Ok(None);
    }
    if runner
        .git(
            repo,
            &[
                "cat-file",
                "-e",
                &format!("{}^{{commit}}", cached.verified_commit),
            ],
        )
        .is_err()
    {
        runner.remaining()?;
        return Ok(None);
    }
    if !support::is_ancestor(runner, repo, &plan.local_commit, &cached.verified_commit)?
        || !support::is_ancestor(runner, repo, &plan.remote_commit, &cached.verified_commit)?
    {
        return Ok(None);
    }
    Ok(Some(cached))
}

pub(super) fn store(
    repo: &Path,
    plan: &Plan,
    verified_commit: &str,
    verification: &[String],
    runner: &Runner,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        plan.required_verification
            .iter()
            .all(|required| verification.contains(required)),
        "Cannot cache a baseline without every required check"
    );
    anyhow::ensure!(
        support::is_ancestor(runner, repo, &plan.local_commit, verified_commit)?
            && support::is_ancestor(runner, repo, &plan.remote_commit, verified_commit)?,
        "Cannot cache a baseline that does not contain both source commits"
    );
    let key = cache_key(plan);
    runner.git(repo, &["update-ref", &cache_ref(&key), verified_commit])?;
    let cached = VerifiedBase {
        schema_version: CACHE_SCHEMA,
        base: plan.base.clone(),
        local_commit: plan.local_commit.clone(),
        remote_commit: plan.remote_commit.clone(),
        common_base: plan.common_base.clone(),
        verified_commit: verified_commit.to_owned(),
        verification: verification.to_vec(),
    };
    crate::artifacts::atomic_write_bytes(
        &cache_directory(repo)?.join(format!("{key}.json")),
        &serde_json::to_vec_pretty(&cached)?,
    )
}

pub(super) fn adopt(
    repo: &Path,
    state: &Implementation,
    plan: &Plan,
    cached: &VerifiedBase,
    runner: &Runner,
) -> anyhow::Result<bool> {
    let branch_ref = format!("refs/heads/{}", state.branch);
    if !state.worktree.exists() {
        let current = match runner.git(repo, &["rev-parse", "--verify", &branch_ref]) {
            Ok(commit) => Some(commit),
            Err(_) => {
                runner.remaining()?;
                None
            }
        };
        if let Some(current) = current {
            if current == plan.remote_commit {
                runner.git(
                    repo,
                    &["update-ref", &branch_ref, &cached.verified_commit, &current],
                )?;
            } else if current != cached.verified_commit {
                return Ok(false);
            }
        }
        return Ok(true);
    }

    anyhow::ensure!(
        common(&state.worktree)?.canonicalize()? == common(repo)?.canonicalize()?
            && runner.git(&state.worktree, &["symbolic-ref", "--short", "HEAD"])? == state.branch,
        "Existing worktree identity changed; refusing to adopt a cached baseline"
    );
    let head = runner.git(&state.worktree, &["rev-parse", "HEAD"])?;
    let merge_head = match runner.git(&state.worktree, &["rev-parse", "--verify", "MERGE_HEAD"]) {
        Ok(commit) => Some(commit),
        Err(_) => {
            runner.remaining()?;
            None
        }
    };
    if let Some(merge_head) = merge_head {
        if head != plan.remote_commit || merge_head != plan.local_commit {
            return Ok(false);
        }
        if !support::unmerged_paths(runner, &state.worktree)?.is_empty()
            || !runner
                .git(&state.worktree, &["diff", "--name-only"])?
                .is_empty()
            || !runner
                .git(
                    &state.worktree,
                    &["ls-files", "--others", "--exclude-standard", "-z"],
                )?
                .is_empty()
        {
            return Ok(false);
        }
        let current_tree = runner.git(&state.worktree, &["write-tree"])?;
        let verified_tree = runner.git(
            repo,
            &[
                "rev-parse",
                "--verify",
                &format!("{}^{{tree}}", cached.verified_commit),
            ],
        )?;
        if current_tree != verified_tree {
            return Ok(false);
        }
        runner.git(
            &state.worktree,
            &["reset", "--hard", &cached.verified_commit],
        )?;
        return Ok(runner
            .git(&state.worktree, &["status", "--porcelain"])?
            .is_empty());
    }

    if !runner
        .git(&state.worktree, &["status", "--porcelain"])?
        .is_empty()
    {
        return Ok(false);
    }
    if head == cached.verified_commit {
        return Ok(true);
    }
    if head != plan.remote_commit {
        return Ok(false);
    }
    runner.git(
        &state.worktree,
        &["merge", "--ff-only", &cached.verified_commit],
    )?;
    anyhow::ensure!(
        runner.git(&state.worktree, &["rev-parse", "HEAD"])? == cached.verified_commit,
        "Cached baseline fast-forward did not reach its verified commit"
    );
    anyhow::ensure!(
        runner
            .git(&state.worktree, &["status", "--porcelain"])?
            .is_empty(),
        "Cached baseline fast-forward left worktree changes; preserving them for review"
    );
    let index_tree = runner.git(&state.worktree, &["write-tree"])?;
    let commit_tree = runner.git(
        &state.worktree,
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{tree}}", cached.verified_commit),
        ],
    )?;
    anyhow::ensure!(
        index_tree == commit_tree,
        "Cached baseline fast-forward left an unexpected index; preserving it for review"
    );
    Ok(true)
}

fn cache_directory(repo: &Path) -> anyhow::Result<PathBuf> {
    Ok(common(repo)?.join("koolade").join("reconciliation-cache"))
}

fn cache_key(plan: &Plan) -> String {
    format!(
        "{:016x}-{}-{}-{}",
        crate::persistence::fnv1a64(plan.base.as_bytes()),
        plan.common_base,
        plan.local_commit,
        plan.remote_commit
    )
}

fn cache_ref(key: &str) -> String {
    format!("refs/koolade-reconciliations/shared/{key}")
}
