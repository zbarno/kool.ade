use super::super::{Implementation, Runner, state_paths::common};
use super::{Plan, support};
use crate::core::implementation::task_repository;
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

mod adopt;
pub(in crate::core::implementation) use adopt::adopt;

const CACHE_SCHEMA: u8 = 1;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::core::implementation) struct VerifiedBase {
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

pub(in crate::core::implementation) fn load(
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

pub(super) fn import_verified_result(
    repo: &Path,
    state: &Implementation,
    commit: &str,
    runner: &Runner,
) -> anyhow::Result<()> {
    if state.task_repository_kind != super::super::TaskRepositoryKind::Clone {
        return Ok(());
    }
    let task_path = state
        .task_repository
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Non-UTF8 task repository path"))?;
    let reference = format!(
        "refs/koolade-reconciliation-results/{}/{}",
        task_repository::allocation_key(state),
        commit
    );
    let source = format!("+refs/heads/{}:{reference}", state.branch);
    runner.git(
        repo,
        &[
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            task_path,
            &source,
        ],
    )?;
    anyhow::ensure!(
        runner.git(repo, &["rev-parse", "--verify", &reference])? == commit,
        "Task repository's reconciliation commit changed before it could be saved"
    );
    Ok(())
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
