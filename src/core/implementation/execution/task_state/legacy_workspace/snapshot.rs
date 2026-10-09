use super::copy;
use crate::core::implementation::Runner;
use std::{
    fs,
    path::{Path, PathBuf},
};

mod index;
mod patch;
mod runtime_configuration;
mod verify;
use patch::{apply_patch, check_diff, files_equal, write_diff};
use runtime_configuration::{
    capture as capture_runtime_configuration, matches as runtime_configuration_matches,
};

const STAGED: &str = "staged.patch";
const UNSTAGED: &str = "unstaged.patch";
const UNTRACKED: &str = "untracked.list";
const RUNTIME_CONFIG: &str = "runtime-config.list";
const IGNORED: &str = "ignored.list";
pub(super) use verify::matches_snapshot;

pub(super) fn index_reference(commit: &str) -> String {
    index::reference(commit)
}

pub(super) fn capture(
    source: &Path,
    migration_dir: &Path,
    head: &str,
    repo: &Path,
    preserve_ignored: bool,
    runner: &Runner,
) -> anyhow::Result<Option<String>> {
    anyhow::ensure!(
        runner.git(source, &["rev-parse", "HEAD"])? == head,
        "Original legacy workspace changed while migration was starting"
    );
    let unmerged = migration_dir.join("unmerged.list");
    runner.git_to_file(source, &["ls-files", "-u", "-z"], &unmerged)?;
    anyhow::ensure!(
        fs::metadata(&unmerged)?.len() == 0,
        "Legacy workspace contains an unresolved index. Resolve it in the original workspace before migrating"
    );
    let snapshot = snapshot_dir(migration_dir);
    ensure_real_directory(&snapshot)?;
    write_diff(
        source,
        &snapshot.join(STAGED),
        &[
            "diff",
            "--cached",
            "--binary",
            "--no-ext-diff",
            "--no-renames",
            "HEAD",
        ],
        runner,
    )?;
    write_diff(
        source,
        &snapshot.join(UNSTAGED),
        &["diff", "--binary", "--no-ext-diff", "--no-renames"],
        runner,
    )?;
    let list = snapshot.join(UNTRACKED);
    runner.git_to_file(
        source,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        &list,
    )?;
    let untracked_root = snapshot.join("untracked");
    ensure_real_directory(&untracked_root)?;
    copy::copy_untracked(source, &untracked_root, &list, true)?;
    capture_runtime_configuration(source, &snapshot)?;
    let staged_index_commit = if preserve_ignored {
        Some(index::capture(source, head, runner)?)
    } else {
        None
    };
    if preserve_ignored {
        let ignored_list = snapshot.join(IGNORED);
        runner.git_to_file(
            source,
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "-z",
            ],
            &ignored_list,
        )?;
        copy::ensure_snapshot_budget(source, &ignored_list)?;
        let ignored_root = snapshot.join("ignored");
        ensure_real_directory(&ignored_root)?;
        copy::copy_untracked(source, &ignored_root, &ignored_list, true)?;
    }
    // Also pin the source commit in the app-owned cache before any caller can
    // change or remove the original linked worktree.
    runner.git(repo, &["cat-file", "-e", &format!("{head}^{{commit}}")])?;
    Ok(staged_index_commit)
}

pub(super) fn ensure_source_matches(
    source: &Path,
    migration_dir: &Path,
    head: &str,
    preserve_ignored: bool,
    staged_index_commit: Option<&str>,
    runner: &Runner,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        runner.git(source, &["rev-parse", "HEAD"])? == head,
        "Original legacy workspace changed during migration; both copies are preserved"
    );
    let snapshot = snapshot_dir(migration_dir);
    let verify = snapshot.join("source-check");
    ensure_real_directory(&verify)?;
    check_diff(
        source,
        &verify.join(STAGED),
        &snapshot.join(STAGED),
        &[
            "diff",
            "--cached",
            "--binary",
            "--no-ext-diff",
            "--no-renames",
            "HEAD",
        ],
        runner,
    )?;
    check_diff(
        source,
        &verify.join(UNSTAGED),
        &snapshot.join(UNSTAGED),
        &["diff", "--binary", "--no-ext-diff", "--no-renames"],
        runner,
    )?;
    let current_list = verify.join(UNTRACKED);
    runner.git_to_file(
        source,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        &current_list,
    )?;
    anyhow::ensure!(
        files_equal(&current_list, &snapshot.join(UNTRACKED))?
            && copy::untracked_match(source, &snapshot.join("untracked"), &current_list)?,
        "Original untracked files changed during migration; both copies are preserved"
    );
    anyhow::ensure!(
        runtime_configuration_matches(source, &snapshot)?,
        "Original granted runtime configuration changed during migration; both copies are preserved"
    );
    if let Some(commit) = staged_index_commit {
        anyhow::ensure!(
            index::matches_source(source, commit, runner)?,
            "Original staged merge index changed during migration; both copies are preserved"
        );
    }
    if preserve_ignored {
        let current_ignored = verify.join(IGNORED);
        runner.git_to_file(
            source,
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "-z",
            ],
            &current_ignored,
        )?;
        anyhow::ensure!(
            files_equal(&current_ignored, &snapshot.join(IGNORED))?
                && copy::untracked_match(source, &snapshot.join("ignored"), &current_ignored)?,
            "Original ignored files changed during merge migration; both copies are preserved"
        );
    }
    Ok(())
}

pub(super) fn restore(
    destination: &Path,
    migration_dir: &Path,
    preserve_ignored: bool,
    staged_index_commit: Option<&str>,
    cache: &Path,
    runner: &Runner,
) -> anyhow::Result<()> {
    let snapshot = snapshot_dir(migration_dir);
    if let Some(commit) = staged_index_commit {
        index::restore(destination, cache, commit, runner)?;
    } else {
        apply_patch(
            destination,
            &snapshot.join(STAGED),
            &snapshot.join("destination-staged-check.patch"),
            &[
                "diff",
                "--cached",
                "--binary",
                "--no-ext-diff",
                "--no-renames",
                "HEAD",
            ],
            true,
            runner,
        )?;
    }
    apply_patch(
        destination,
        &snapshot.join(UNSTAGED),
        &snapshot.join("destination-unstaged-check.patch"),
        &["diff", "--binary", "--no-ext-diff", "--no-renames"],
        false,
        runner,
    )?;
    copy::copy_untracked(
        &snapshot.join("untracked"),
        destination,
        &snapshot.join(UNTRACKED),
        false,
    )?;
    copy::copy_untracked(
        &snapshot.join("runtime-config"),
        destination,
        &snapshot.join(RUNTIME_CONFIG),
        false,
    )?;
    if preserve_ignored {
        copy::copy_untracked(
            &snapshot.join("ignored"),
            destination,
            &snapshot.join(IGNORED),
            false,
        )?;
    }
    Ok(())
}

pub(super) fn snapshot_dir(migration_dir: &Path) -> PathBuf {
    migration_dir.join("snapshot")
}

pub(super) fn ensure_real_directory(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Migration snapshot path is not a real directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path)?,
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
