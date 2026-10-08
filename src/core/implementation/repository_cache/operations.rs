use super::Runner;
use std::{fs, path::Path};

pub(super) fn ensure_bare_cache(
    source: &Path,
    cache: &Path,
    origin: Option<&str>,
    push: Option<&str>,
    fetch: Option<&str>,
    runner: &Runner,
) -> anyhow::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(cache) {
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Repository cache path is not a real directory"
        );
        configure_remote(cache, origin, push, fetch, runner)?;
        verify_bare_cache(cache, runner)?;
        return Ok(());
    }
    let parent = cache
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Repository cache has no parent"))?;
    fs::create_dir_all(parent)?;
    let temp = temporary_sibling(cache, "creating");
    let source = source
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Non-UTF8 source repository path"))?;
    let result = (|| {
        runner.git(
            Path::new(source),
            &[
                "clone",
                "--bare",
                "--no-hardlinks",
                source,
                path_text(&temp)?,
            ],
        )?;
        configure_remote(&temp, origin, push, fetch, runner)?;
        verify_bare_cache(&temp, runner)?;
        fs::rename(&temp, cache)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temp);
    }
    result
}

pub(super) fn create_independent_clone(
    cache: &Path,
    request: CloneRequest<'_>,
    runner: &Runner,
) -> anyhow::Result<()> {
    let CloneRequest {
        source_branch,
        source_commit,
        task_branch,
        destination,
        commit_identity,
        remotes,
    } = request;
    anyhow::ensure!(
        !destination.exists(),
        "Task repository path already exists; preserve it for review"
    );
    runner.git(
        cache,
        &["cat-file", "-e", &format!("{source_commit}^{{commit}}")],
    )?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = temporary_sibling(destination, "creating");
    let cache = path_text(cache)?;
    let temp_text = path_text(&temp)?;
    let result = (|| {
        runner.git_with_reflog_identity(
            Path::new(cache),
            &[
                "clone",
                "--no-hardlinks",
                "--no-local",
                "--no-checkout",
                "--single-branch",
                "--branch",
                source_branch,
                cache,
                temp_text,
            ],
        )?;
        anyhow::ensure!(
            runner.git(&temp, &["rev-parse", "HEAD"])? == source_commit,
            "Repository cache source ref moved during clone creation"
        );
        configure_remote(&temp, remotes.origin, remotes.push, None, runner)?;
        super::commit_identity::configure_clone(&temp, commit_identity, false, runner)?;
        // Creating the task branch writes a local reflog entry. Supply an
        // explicitly local-only identity so Git does not try to infer an
        // email from the machine hostname when the ambient identity is absent.
        runner.git_with_reflog_identity(&temp, &["checkout", "-B", task_branch, source_commit])?;
        anyhow::ensure!(
            runner.git(&temp, &["symbolic-ref", "--short", "HEAD"])? == task_branch
                && runner.git(&temp, &["rev-parse", "HEAD"])? == source_commit,
            "Task repository did not start at the pinned commit on its assigned branch"
        );
        verify_independent_repository(&temp, runner)?;
        fs::rename(&temp, destination)?;
        verify_independent_repository(destination, runner)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temp);
    }
    result
}

pub(super) struct CloneRemotes<'a> {
    pub(super) origin: Option<&'a str>,
    pub(super) push: Option<&'a str>,
}

pub(super) struct CloneRequest<'a> {
    pub(super) source_branch: &'a str,
    pub(super) source_commit: &'a str,
    pub(super) task_branch: &'a str,
    pub(super) destination: &'a Path,
    pub(super) commit_identity: &'a super::GitCommitIdentity,
    pub(super) remotes: CloneRemotes<'a>,
}

pub(super) fn verify_independent_repository(path: &Path, runner: &Runner) -> anyhow::Result<()> {
    let root = path.canonicalize()?;
    let git_entry = root.join(".git");
    let metadata = fs::symlink_metadata(&git_entry)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "Task repository must have its own .git directory"
    );
    let git_entry = git_entry.canonicalize()?;
    let top = runner.git(
        &root,
        &["rev-parse", "--path-format=absolute", "--show-toplevel"],
    )?;
    let admin = runner.git(&root, &["rev-parse", "--path-format=absolute", "--git-dir"])?;
    let common = runner.git(
        &root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    anyhow::ensure!(
        Path::new(&top).canonicalize()? == root
            && Path::new(&admin).canonicalize()? == git_entry
            && Path::new(&common).canonicalize()? == git_entry,
        "Task repository shares Git metadata with another checkout"
    );
    anyhow::ensure!(
        !git_entry.join("objects/info/alternates").exists(),
        "Task repository uses an external Git object store"
    );
    anyhow::ensure!(
        runner.git(&root, &["rev-parse", "--is-shallow-repository"])? == "false",
        "Task repository must contain complete history"
    );
    Ok(())
}

pub(super) fn verify_bare_cache(path: &Path, runner: &Runner) -> anyhow::Result<()> {
    anyhow::ensure!(
        runner.git(path, &["rev-parse", "--is-bare-repository"])? == "true",
        "Repository cache is not a bare Git repository"
    );
    anyhow::ensure!(
        !path.join("objects/info/alternates").exists(),
        "Repository cache must not borrow another object store"
    );
    Ok(())
}

fn configure_remote(
    path: &Path,
    origin: Option<&str>,
    push: Option<&str>,
    fetch: Option<&str>,
    runner: &Runner,
) -> anyhow::Result<()> {
    if let Some(origin) = origin {
        if runner.git(path, &["remote", "get-url", "origin"]).is_ok() {
            runner.git(path, &["remote", "set-url", "origin", origin])?;
        } else {
            runner.git(path, &["remote", "add", "origin", origin])?;
        }
        if let Some(push) = push.filter(|push| *push != origin) {
            runner.git(
                path,
                &["config", "--replace-all", "remote.origin.pushurl", push],
            )?;
        } else {
            let _ = runner.git(path, &["config", "--unset-all", "remote.origin.pushurl"]);
        }
    } else if runner.git(path, &["remote", "get-url", "origin"]).is_ok() {
        runner.git(path, &["remote", "remove", "origin"])?;
    }
    if let Some(fetch) = fetch {
        runner.git(
            path,
            &["config", "--replace-all", "koolade.fetchUrl", fetch],
        )?;
    } else {
        let _ = runner.git(path, &["config", "--unset-all", "koolade.fetchUrl"]);
    }
    Ok(())
}

fn temporary_sibling(path: &Path, purpose: &str) -> std::path::PathBuf {
    path.with_file_name(format!(
        ".{}-{purpose}-{}-{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ))
}

fn path_text(path: &Path) -> anyhow::Result<&str> {
    path.to_str()
        .ok_or_else(|| anyhow::anyhow!("Non-UTF8 repository path"))
}
