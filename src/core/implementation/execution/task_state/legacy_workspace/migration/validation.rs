use super::merge_state::read_merge_head;
use crate::core::implementation::{Implementation, Runner, task_repository};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn validate_legacy_workspace(
    source: &Path,
    repo: &Path,
    state: &Implementation,
    runner: &Runner,
) -> anyhow::Result<Option<String>> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| anyhow::anyhow!("Saved legacy workspace is unavailable: {error}"))?;
    anyhow::ensure!(metadata.is_dir() && !metadata.file_type().is_symlink());
    let expected_root = repo
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Project repository has no parent"))?
        .join(".koolade-worktrees")
        .join(crate::persistence::project_slug(&repo.canonicalize()?));
    let actual_name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Legacy workspace path is invalid"))?;
    let allocation = task_repository::allocation_key(state);
    let valid_name = actual_name == allocation
        || actual_name
            .strip_prefix(&format!("{allocation}-integration-"))
            .is_some_and(|suffix| {
                suffix.len() == 12 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
    let expected_branch = if actual_name == allocation {
        format!("koolade/{allocation}")
    } else {
        let suffix = actual_name
            .strip_prefix(&format!("{allocation}-integration-"))
            .unwrap_or_default();
        format!("koolade/integration/{allocation}/{suffix}")
    };
    anyhow::ensure!(
        source.parent().is_some_and(|parent| {
            parent.canonicalize().ok() == expected_root.canonicalize().ok()
        }) && valid_name
            && state.branch == expected_branch,
        "Saved legacy workspace is outside the Kool.ad/e worktree root"
    );
    let top = PathBuf::from(runner.git(
        source,
        &["rev-parse", "--path-format=absolute", "--show-toplevel"],
    )?);
    let common = PathBuf::from(runner.git(
        source,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    let repo_common = PathBuf::from(runner.git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    anyhow::ensure!(
        top.canonicalize()? == source.canonicalize()?
            && common.canonicalize()? == repo_common.canonicalize()?
            && runner.git(source, &["symbolic-ref", "--short", "HEAD"])? == state.branch,
        "Saved legacy workspace ownership or branch metadata is unexpected; preserve it for review"
    );
    anyhow::ensure!(
        runner.git(source, &["ls-files", "-u", "-z"])?.is_empty(),
        "Legacy workspace contains an unresolved index. Resolve it in the original workspace, then retry migration"
    );
    let git_dir = PathBuf::from(runner.git(source, &["rev-parse", "--absolute-git-dir"])?);
    for marker in [
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "REBASE_HEAD",
        "rebase-apply",
        "rebase-merge",
        "sequencer",
    ] {
        match fs::symlink_metadata(git_dir.join(marker)) {
            Ok(_) => anyhow::bail!(
                "Legacy workspace has an unfinished Git operation ({marker}). Complete or abort it in the original workspace, then retry migration; the original is preserved"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    read_merge_head(source, runner)
}
