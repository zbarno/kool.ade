mod history;

use super::super::*;
use std::path::Path;

pub(super) const SNAPSHOT_FILE: &str = "base-reconciliation-recovery.json";

#[derive(serde::Serialize)]
struct Snapshot {
    schema_version: u8,
    phase: String,
    worktree: String,
    branch: String,
    base: String,
    target: String,
    merge_head: String,
    staged_paths: Vec<String>,
    unstaged_paths: Vec<String>,
    untracked_paths: Vec<String>,
    ignored_paths: Vec<String>,
    stash_commit: Option<String>,
    private_ref: Option<String>,
    detail: Option<String>,
}

pub(super) fn recover_unexpected_merge(
    repo: &Path,
    dir: &Path,
    state: &Implementation,
    runner: &Runner,
    plan: &Plan,
    merge_head: &str,
    explicit_resume: bool,
) -> anyhow::Result<()> {
    let snapshot_path = dir.join(SNAPSHOT_FILE);
    if snapshot_path.exists() {
        anyhow::ensure!(
            explicit_resume,
            "{}",
            review_required(
                state,
                plan,
                &snapshot_path,
                "The prior recovery is retained. Explicitly resume to authorize one fresh preservation attempt."
            )
        );
        history::archive(repo, &snapshot_path, state, runner, plan)?;
    }

    let status = Snapshot {
        schema_version: 1,
        phase: "snapshot_pending".into(),
        worktree: state.worktree.display().to_string(),
        branch: state.branch.clone(),
        base: plan.remote_commit.clone(),
        target: plan.local_commit.clone(),
        merge_head: merge_head.into(),
        staged_paths: paths(
            runner,
            &state.worktree,
            &["diff", "--cached", "--name-only", "-z"],
        )?,
        unstaged_paths: paths(runner, &state.worktree, &["diff", "--name-only", "-z"])?,
        untracked_paths: paths(
            runner,
            &state.worktree,
            &["ls-files", "--others", "--exclude-standard", "-z"],
        )?,
        ignored_paths: paths(
            runner,
            &state.worktree,
            &[
                "ls-files",
                "--others",
                "--ignored",
                "--exclude-standard",
                "-z",
            ],
        )?,
        stash_commit: None,
        private_ref: None,
        detail: None,
    };
    write_snapshot(&snapshot_path, &status)?;

    let nonce = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let marker = format!(
        "koolade-reconciliation:{}:{nonce}",
        key_for_ticket(&state.ticket)
    );
    if let Err(error) = runner.git(
        &state.worktree,
        &["stash", "push", "--all", "--message", &marker],
    ) {
        let mut failed = status;
        failed.phase = "snapshot_failed".into();
        failed.detail = Some(format!(
            "Could not preserve the unexpected changes: {error:#}"
        ));
        write_snapshot(&snapshot_path, &failed)?;
        return Err(review_required(
            state,
            plan,
            &snapshot_path,
            "Automatic recovery could not create a complete Git snapshot; the worktree is preserved.",
        ));
    }

    let stash_list = runner.git(&state.worktree, &["stash", "list", "--format=%H%x09%gs"])?;
    let stash_commit = stash_list
        .lines()
        .find(|line| line.contains(&marker))
        .and_then(|line| line.split_once('\t'))
        .map(|(commit, _)| commit.to_owned());
    let Some(stash_commit) = stash_commit else {
        let mut failed = status;
        failed.phase = "snapshot_missing".into();
        failed.detail = Some("Git did not record the requested all-files stash.".into());
        write_snapshot(&snapshot_path, &failed)?;
        return Err(review_required(
            state,
            plan,
            &snapshot_path,
            "Git did not expose a durable all-files snapshot; the merge was not retried.",
        ));
    };
    let private_ref = format!(
        "refs/koolade/reconciliation-recovery/{}/{nonce}",
        key_for_ticket(&state.ticket)
    );
    runner.git(repo, &["update-ref", &private_ref, &stash_commit])?;

    let mut saved = status;
    saved.phase = "snapshot_saved".into();
    saved.stash_commit = Some(stash_commit);
    saved.private_ref = Some(private_ref);
    write_snapshot(&snapshot_path, &saved)?;

    if super::auto_verify::current_merge_head(runner, &state.worktree)?.is_some() {
        runner.git(&state.worktree, &["merge", "--abort"])?;
    }
    let clean = runner.git(
        &state.worktree,
        &[
            "status",
            "--porcelain",
            "--ignored",
            "--untracked-files=all",
        ],
    )?;
    anyhow::ensure!(
        runner.git(&state.worktree, &["rev-parse", "HEAD"])? == plan.remote_commit
            && super::auto_verify::current_merge_head(runner, &state.worktree)?.is_none()
            && clean.is_empty(),
        "The saved reconciliation worktree could not be restored to its recorded clean base; its recovery snapshot is at {}",
        snapshot_path.display()
    );

    runner.update(format!(
        "Preserved unexpected merge changes at {}; retrying the pinned merge once…",
        snapshot_path.display()
    ));
    if let Err(error) = runner.git(
        &state.worktree,
        &[
            "merge",
            "--no-ff",
            "--no-commit",
            "--no-edit",
            &plan.local_commit,
        ],
    ) {
        let mut failed = saved;
        failed.phase = "retry_failed".into();
        failed.detail = Some(format!("The one clean-merge retry failed: {error:#}"));
        write_snapshot(&snapshot_path, &failed)?;
        return Err(review_required(
            state,
            plan,
            &snapshot_path,
            "The pinned merge still needs review after one recovery attempt.",
        ));
    }
    let mut recovered = saved;
    recovered.phase = "recovered_once".into();
    write_snapshot(&snapshot_path, &recovered)?;
    Ok(())
}

fn paths(runner: &Runner, worktree: &Path, args: &[&str]) -> anyhow::Result<Vec<String>> {
    Ok(runner
        .git(worktree, args)?
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect())
}

fn write_snapshot(path: &Path, snapshot: &Snapshot) -> anyhow::Result<()> {
    crate::artifacts::atomic_write_bytes(path, &serde_json::to_vec_pretty(snapshot)?)
}

fn review_required(
    state: &Implementation,
    plan: &Plan,
    path: &Path,
    reason: &str,
) -> anyhow::Error {
    let changes = fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .unwrap_or_default();
    let count = |field: &str| {
        changes
            .get(field)
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len)
    };
    super::super::support::user_action(format!(
        "Reconciliation recovery stopped for {} on branch {} at base {} targeting {}. {reason} Changes: {} staged, {} unstaged, {} untracked, {} ignored. Recovery snapshot: {}. The task worktree and both pinned commits are retained; inspect the snapshot before choosing whether to restore it or keep the work area for review.",
        state.ticket,
        state.branch,
        plan.remote_commit,
        plan.local_commit,
        count("staged_paths"),
        count("unstaged_paths"),
        count("untracked_paths"),
        count("ignored_paths"),
        path.display()
    ))
}
