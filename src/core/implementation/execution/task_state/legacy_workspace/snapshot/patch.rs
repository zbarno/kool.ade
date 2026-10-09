use crate::core::implementation::Runner;
use std::{
    fs::{self, OpenOptions},
    io::Read,
    path::Path,
};

use super::ensure_real_directory;

pub(super) fn apply_patch(
    repo: &Path,
    patch: &Path,
    current: &Path,
    diff_args: &[&str],
    staged: bool,
    runner: &Runner,
) -> anyhow::Result<()> {
    let parent = current.parent().unwrap();
    ensure_real_directory(parent)?;
    let output_arg = format!("--output={}", current.display());
    let mut args = diff_args.to_vec();
    args.push(&output_arg);
    runner.git(repo, &args)?;
    if files_equal(current, patch)? {
        return Ok(());
    }
    anyhow::ensure!(
        fs::metadata(current)?.len() == 0,
        "Migrated clone already contains different staged or unstaged edits; both copies are preserved"
    );
    if fs::metadata(patch)?.len() == 0 {
        return Ok(());
    }
    let patch_path = patch
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Non-UTF8 migration patch path"))?;
    if staged {
        runner.git(repo, &["apply", "--index", "--binary", patch_path])?;
    } else {
        runner.git(repo, &["apply", "--binary", patch_path])?;
    }
    Ok(())
}

pub(super) fn check_diff(
    repo: &Path,
    current: &Path,
    expected: &Path,
    args: &[&str],
    runner: &Runner,
) -> anyhow::Result<()> {
    write_diff(repo, current, args, runner)?;
    anyhow::ensure!(
        files_equal(current, expected)?,
        "Original legacy workspace changed during migration; both copies are preserved"
    );
    Ok(())
}

pub(super) fn write_diff(
    repo: &Path,
    output: &Path,
    args: &[&str],
    runner: &Runner,
) -> anyhow::Result<()> {
    ensure_real_directory(output.parent().unwrap())?;
    ensure_regular_output(output)?;
    let output_path = output
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Non-UTF8 migration output path"))?;
    let output_arg = format!("--output={output_path}");
    let mut args = args.to_vec();
    args.push(&output_arg);
    runner.git(repo, &args)?;
    Ok(())
}

fn ensure_regular_output(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Migration output path is not a regular file"
            );
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            OpenOptions::new().write(true).create_new(true).open(path)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

pub(super) fn files_equal(left: &Path, right: &Path) -> anyhow::Result<bool> {
    let left_meta = fs::symlink_metadata(left)?;
    let right_meta = fs::symlink_metadata(right)?;
    anyhow::ensure!(
        left_meta.is_file()
            && !left_meta.file_type().is_symlink()
            && right_meta.is_file()
            && !right_meta.file_type().is_symlink(),
        "Migration patch path is not a regular file"
    );
    if left_meta.len() != right_meta.len() {
        return Ok(false);
    }
    let mut left = fs::File::open(left)?;
    let mut right = fs::File::open(right)?;
    let mut left_buffer = [0_u8; 32 * 1024];
    let mut right_buffer = [0_u8; 32 * 1024];
    loop {
        let left_read = left.read(&mut left_buffer)?;
        let right_read = right.read(&mut right_buffer)?;
        if left_read != right_read || left_buffer[..left_read] != right_buffer[..right_read] {
            return Ok(false);
        }
        if left_read == 0 {
            return Ok(true);
        }
    }
}
