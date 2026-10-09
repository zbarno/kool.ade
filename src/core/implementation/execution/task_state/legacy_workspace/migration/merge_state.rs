use crate::core::implementation::Runner;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn read_merge_head(source: &Path, runner: &Runner) -> anyhow::Result<Option<String>> {
    let git_dir = PathBuf::from(runner.git(source, &["rev-parse", "--absolute-git-dir"])?);
    let path = git_dir.join("MERGE_HEAD");
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Legacy merge metadata is not a regular file; the original is preserved for review"
            );
            let contents = fs::read_to_string(path)?;
            let mut heads = contents.lines();
            let head = heads
                .next()
                .filter(|head| {
                    matches!(head.len(), 40 | 64)
                        && head.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
                .ok_or_else(|| anyhow::anyhow!("Legacy merge metadata is invalid"))?;
            anyhow::ensure!(
                heads.next().is_none() && contents.trim_end().lines().count() == 1,
                "Legacy octopus merge state is preserved for manual review"
            );
            Ok(Some(head.to_owned()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn ensure_merge_in_progress(
    destination: &Path,
    cache: &Path,
    merge_head: &str,
    runner: &Runner,
) -> anyhow::Result<()> {
    let git_dir = PathBuf::from(runner.git(destination, &["rev-parse", "--absolute-git-dir"])?);
    let marker = git_dir.join("MERGE_HEAD");
    match fs::symlink_metadata(&marker) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && fs::read_to_string(&marker)?.trim() == merge_head,
                "Partially migrated merge state differs from its durable migration record"
            );
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let cache = cache
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Non-UTF8 repository cache path"))?;
            runner.git(
                destination,
                &[
                    "fetch",
                    "--no-tags",
                    "--no-write-fetch-head",
                    cache,
                    merge_head,
                ],
            )?;
            runner.git(
                destination,
                &["merge", "--no-ff", "--no-commit", "--no-edit", merge_head],
            )?;
        }
        Err(error) => return Err(error.into()),
    }
    anyhow::ensure!(
        runner
            .git_nul_records(destination, &["ls-files", "-u", "-z"])?
            .is_empty()
            && fs::read_to_string(marker)?.trim() == merge_head,
        "Pinned legacy merge could not be reconstructed cleanly; both copies are preserved"
    );
    Ok(())
}
