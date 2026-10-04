//! Path-limited Git checkpoints that preserve unrelated staged changes.
mod index;

use super::{AUTHOR_EMAIL, AUTHOR_NAME, require_exit_success, run};
use crate::error::AppError;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub(super) fn command_error(command: &str, detail: &str) -> AppError {
    AppError::Git {
        cmd: command.into(),
        detail: detail.trim().to_owned(),
    }
}

fn repository_lock(cwd: &Path) -> Result<fs::File, AppError> {
    let (code, common, err) = run(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    require_exit_success("rev-parse --git-common-dir", code, common.clone(), err)?;
    let path = PathBuf::from(common.trim()).join("koolade-planning-commit.lock");
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| command_error("open planning commit lock", &error.to_string()))?;
    file.lock()
        .map_err(|error| command_error("lock planning commits", &error.to_string()))?;
    Ok(file)
}

fn validate_path(path: &str) -> Result<(), AppError> {
    if path.trim().is_empty()
        || !Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(command_error(
            "commit",
            &format!("invalid repository-relative commit path: {path:?}"),
        ));
    }
    Ok(())
}

fn pathspecs(paths: &[String]) -> Result<Vec<String>, AppError> {
    paths
        .iter()
        .map(|path| {
            validate_path(path)?;
            Ok(format!(":(top,literal){path}"))
        })
        .collect()
}

fn short_head(cwd: &Path) -> Result<String, AppError> {
    let (code, output, error) = run(cwd, &["rev-parse", "--short", "HEAD"])?;
    if code == 0 {
        Ok(output.trim().to_owned())
    } else if run(cwd, &["symbolic-ref", "-q", "HEAD"])?.0 != 0 {
        Err(command_error("rev-parse --short HEAD", &error))
    } else {
        Ok(String::new())
    }
}

/// Commit only the supplied paths. Git's `--only` mode updates those index
/// entries and excludes every other staged path from the Koolade commit.
/// The process-shared lock also makes multiple Koolade instances serialize
/// their checkpoints against the latest HEAD.
pub fn commit(cwd: &Path, message: &str, paths: &[String]) -> Result<String, AppError> {
    commit_with_cancel(cwd, message, paths, None)
}

/// Cancellable variant used by long-running planning turns. Once Git's
/// atomic commit command starts, let it finish; cancellation is honored at
/// both safe boundaries before Git mutates or publishes the checkpoint.
pub fn commit_cancellable(
    cwd: &Path,
    message: &str,
    paths: &[String],
    cancel: &AtomicBool,
) -> Result<String, AppError> {
    commit_with_cancel(cwd, message, paths, Some(cancel))
}

fn commit_with_cancel(
    cwd: &Path,
    message: &str,
    paths: &[String],
    cancel: Option<&AtomicBool>,
) -> Result<String, AppError> {
    let _lock = repository_lock(cwd)?;
    if is_cancelled(cancel) {
        return Err(cancelled_error());
    }
    let specs = pathspecs(paths)?;
    if specs.is_empty() {
        return short_head(cwd);
    }

    index::reject_unmerged_paths(cwd, &specs)?;
    let snapshot = index::snapshot(cwd, &specs)?;
    let original_staged = index::staged_paths(cwd, paths, &specs)?;
    let all_authorized = paths.iter().cloned().collect::<BTreeSet<_>>();

    let mut add = vec!["add".to_owned(), "--all".to_owned(), "--".to_owned()];
    add.extend(specs.iter().cloned());
    let add_refs = add.iter().map(String::as_str).collect::<Vec<_>>();
    let (code, stdout, stderr) = run(cwd, &add_refs)?;
    if let Err(error) = require_exit_success("add authorized paths", code, stdout, stderr) {
        return Err(index::preserve_or_report(
            cwd,
            &snapshot,
            &all_authorized,
            error,
        ));
    }
    if is_cancelled(cancel) {
        return Err(index::preserve_or_report(
            cwd,
            &snapshot,
            &all_authorized,
            cancelled_error(),
        ));
    }

    let mut diff = vec![
        "diff".to_owned(),
        "--cached".to_owned(),
        "--quiet".to_owned(),
        "--".to_owned(),
    ];
    diff.extend(specs.iter().cloned());
    let diff_refs = diff.iter().map(String::as_str).collect::<Vec<_>>();
    let (code, stdout, stderr) = run(cwd, &diff_refs)?;
    if code == 0 {
        index::restore_paths(cwd, &snapshot, &all_authorized)?;
        return short_head(cwd);
    }
    if code != 1 {
        let error = command_error("diff --cached", &format!("{stdout}{stderr}"));
        return Err(index::preserve_or_report(
            cwd,
            &snapshot,
            &all_authorized,
            error,
        ));
    }
    if is_cancelled(cancel) {
        return Err(index::preserve_or_report(
            cwd,
            &snapshot,
            &all_authorized,
            cancelled_error(),
        ));
    }

    let mut args = vec![
        "-c".to_owned(),
        format!("user.name={AUTHOR_NAME}"),
        "-c".to_owned(),
        format!("user.email={AUTHOR_EMAIL}"),
        "commit".to_owned(),
        "-q".to_owned(),
        "-m".to_owned(),
        message.to_owned(),
        "--only".to_owned(),
        "--".to_owned(),
    ];
    args.extend(specs);
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let (status, stdout, stderr) = run(cwd, &refs)?;
    if let Err(error) = require_exit_success("commit", status, stdout, stderr) {
        return Err(index::preserve_or_report(
            cwd,
            &snapshot,
            &all_authorized,
            error,
        ));
    }
    index::restore_paths(cwd, &snapshot, &original_staged)?;
    short_head(cwd)
}

fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::SeqCst))
}

fn cancelled_error() -> AppError {
    AppError::Other("Git checkpoint cancelled before Kool.ad/e committed it".into())
}

/// Convenience: commit only canonical planning artifacts.
pub fn commit_planning_changes(
    cwd: &Path,
    message: &str,
    paths: &[String],
) -> Result<String, AppError> {
    commit(cwd, message, paths)
}
