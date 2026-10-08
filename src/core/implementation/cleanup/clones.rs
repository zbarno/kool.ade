use super::*;
use crate::core::implementation::repository_cache::RepositoryCache;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn reclaim(dir: &Path, state: &Implementation, runner: &Runner) -> anyhow::Result<()> {
    let cache = RepositoryCache::from_saved_state(state, runner)?;
    let publish_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(cache.path.join("koolade-auto-publish.lock"))?;
    publish_lock.try_lock().map_err(|_| {
        anyhow::anyhow!("Publication is active; cleanup will retry when it finishes")
    })?;

    let merged = state.merged_commit.as_deref().ok_or_else(|| {
        anyhow::anyhow!("No confirmed completion commit; task repositories preserved")
    })?;
    validate_commit(merged)?;
    let remote = cache.refresh_branch(&state.base, runner)?;
    let task_key = task_repository::allocation_key(state);
    let remote_ref = format!("refs/koolade-cleanup-bases/{task_key}");
    runner.git(&cache.path, &["update-ref", &remote_ref, &remote])?;
    runner
        .git(
            &cache.path,
            &["merge-base", "--is-ancestor", merged, &remote_ref],
        )
        .map_err(|_| {
            anyhow::anyhow!(
                "Completion commit {merged} is not in origin/{}; task repositories preserved",
                state.base
            )
        })?;

    let mut paths = BTreeSet::from_iter(state.task_repositories.iter().cloned());
    paths.insert(state.task_repository.clone());
    let mut commits = state.task_repository_commits.clone();
    if let Some(head) = &state.verified_head {
        commits
            .entry(state.task_repository.to_string_lossy().into_owned())
            .or_insert_with(|| head.clone());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("integration-")
        {
            continue;
        }
        anyhow::ensure!(
            entry.file_type()?.is_dir() && !entry.file_type()?.is_symlink(),
            "Integration evidence directory is not a real directory; task repositories preserved"
        );
        let record = super::super::read_state_file(&entry.path().join("state.json"))?;
        let same_task = record.ticket == state.ticket && record.task_uid == state.task_uid
            || state.task_uid.is_some()
                && record.task_uid == state.task_uid
                && state
                    .task_repository_allocation_key
                    .as_ref()
                    .is_some_and(|key| record.task_repository_allocation_key.as_ref() == Some(key));
        anyhow::ensure!(
            same_task && record.repository_identity == state.repository_identity,
            "Integration identity changed; task repositories preserved"
        );
        for path in record.task_repositories {
            paths.insert(path);
        }
        commits.extend(record.task_repository_commits);
        if let Some(head) = record.verified_head {
            commits
                .entry(record.task_repository.to_string_lossy().into_owned())
                .or_insert(head);
        }
    }

    for path in paths {
        remove_clone(state, &path, &commits, runner)?;
    }
    Ok(())
}

fn remove_clone(
    state: &Implementation,
    path: &Path,
    commits: &BTreeMap<String, String>,
    runner: &Runner,
) -> anyhow::Result<()> {
    let mut record = state.clone();
    if !record.task_repositories.contains(&path.to_path_buf()) {
        record.task_repositories.push(path.to_path_buf());
    }
    task_repository::validate_clone_path_at(&record, path)?;
    let branch = task_repository::branch_for_path(&record, path)?;
    let Some(head) = commits.get(&path.to_string_lossy().into_owned()) else {
        anyhow::bail!(
            "No verified commit is recorded for {}; preserved",
            path.display()
        );
    };
    validate_commit(head)?;

    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Task repository path is not a real directory; preserved"
    );
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Task repository has no allocation parent"))?;
    let parent_metadata = fs::symlink_metadata(parent)?;
    anyhow::ensure!(
        parent_metadata.is_dir() && !parent_metadata.file_type().is_symlink(),
        "Task repository allocation parent is not a real directory; preserved"
    );
    anyhow::ensure!(
        path.canonicalize()? == parent.canonicalize()?.join(path.file_name().unwrap()),
        "Task repository path changed; preserved"
    );
    RepositoryCache::verify_task_repository(path, runner)?;
    anyhow::ensure!(
        runner.git(path, &["symbolic-ref", "--short", "HEAD"])? == branch,
        "Task repository branch changed; preserved"
    );
    let actual = runner.git(path, &["rev-parse", "HEAD"])?;
    anyhow::ensure!(
        actual == *head,
        "Task repository has an unverified HEAD; preserved"
    );
    anyhow::ensure!(
        runner
            .git(path, &["status", "--porcelain", "--untracked-files=all"])?
            .is_empty(),
        "Task repository contains changes or untracked files; preserved for review"
    );
    let cache = RepositoryCache::from_saved_state(state, runner)?;
    cache.pin_evidence(&task_repository::allocation_key(state), head, runner)?;
    fs::remove_dir_all(path)?;
    Ok(())
}

fn validate_commit(commit: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(commit.len(), 40 | 64) && commit.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid commit identity; task repositories preserved"
    );
    Ok(())
}
