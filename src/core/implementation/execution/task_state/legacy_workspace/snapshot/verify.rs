use super::{
    IGNORED, RUNTIME_CONFIG, STAGED, UNSTAGED, UNTRACKED, copy, ensure_real_directory, files_equal,
    snapshot_dir, write_diff,
};
use crate::core::implementation::Runner;
use std::path::Path;

pub(in crate::core::implementation::execution::task_state::legacy_workspace) fn matches_snapshot(
    destination: &Path,
    migration_dir: &Path,
    preserve_ignored: bool,
    runner: &Runner,
) -> anyhow::Result<bool> {
    let snapshot = snapshot_dir(migration_dir);
    let verify = snapshot.join("destination-check");
    ensure_real_directory(&verify)?;
    write_diff(
        destination,
        &verify.join(STAGED),
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
        destination,
        &verify.join(UNSTAGED),
        &["diff", "--binary", "--no-ext-diff", "--no-renames"],
        runner,
    )?;
    let list = verify.join(UNTRACKED);
    runner.git_to_file(
        destination,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        &list,
    )?;
    let mut matches = files_equal(&verify.join(STAGED), &snapshot.join(STAGED))?
        && files_equal(&verify.join(UNSTAGED), &snapshot.join(UNSTAGED))?
        && files_equal(&list, &snapshot.join(UNTRACKED))?
        && copy::untracked_match(destination, &snapshot.join("untracked"), &list)?
        && copy::untracked_match(
            destination,
            &snapshot.join("runtime-config"),
            &snapshot.join(RUNTIME_CONFIG),
        )?;
    if preserve_ignored {
        let ignored = verify.join(IGNORED);
        runner.git_to_file(
            destination,
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "-z",
            ],
            &ignored,
        )?;
        matches &= files_equal(&ignored, &snapshot.join(IGNORED))?
            && copy::untracked_match(destination, &snapshot.join("ignored"), &ignored)?;
    }
    Ok(matches)
}
