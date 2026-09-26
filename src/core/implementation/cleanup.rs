//! Conservative, restartable reclamation of completed task worktrees.
//! Reports stay in the implementation directory; cleanup never resets or forces Git.
use super::*;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cleanup {
    pub completed_at: Option<String>,
    pub attempted_at: Option<String>,
    pub error: Option<String>,
}

/// Called with the ticket lock held. Cleanup failure must never undo completion.
pub(super) fn run(repo: &Path, dir: &Path, state: &mut Implementation, runner: &Runner) {
    if state.status != super::ImplementationStatus::Completed
        || state.cleanup.completed_at.is_some()
    {
        return;
    }
    state.cleanup.attempted_at = Some(chrono::Utc::now().to_rfc3339());
    match reclaim(repo, dir, state, runner) {
        Ok(()) => {
            state.cleanup.error = None;
            state.cleanup.completed_at = Some(chrono::Utc::now().to_rfc3339());
        }
        Err(error) => state.cleanup.error = Some(format!("{error:#}")),
    }
}

fn reclaim(repo: &Path, dir: &Path, state: &Implementation, runner: &Runner) -> anyhow::Result<()> {
    let publish_lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(common(repo)?.join("packet-auto-publish.lock"))?;
    publish_lock.try_lock().map_err(|_| {
        anyhow::anyhow!("Publication is active; cleanup will retry when it finishes")
    })?;
    let merged = state
        .merged_commit
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("No confirmed completion commit; worktrees preserved"))?;
    anyhow::ensure!(
        matches!(merged.len(), 40 | 64) && merged.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid completion commit; worktrees preserved"
    );
    let remote_ref = format!("refs/packet-cleanup-bases/{}", key(&state.ticket));
    runner.git(
        repo,
        &[
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            "origin",
            &format!("+refs/heads/{}:{remote_ref}", state.base),
        ],
    )?;
    runner
        .git(repo, &["merge-base", "--is-ancestor", merged, &remote_ref])
        .map_err(|_| {
            anyhow::anyhow!(
                "Completion commit {merged} is not in origin/{}; worktrees preserved",
                state.base
            )
        })?;

    let mut records = vec![(
        state.clone(),
        key(&state.ticket),
        format!("packet/{}", key(&state.ticket)),
    )];
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
            !entry.file_type()?.is_symlink(),
            "Integration evidence directory is a symlink; preserved"
        );
        let record = super::read_state_file(&entry.path().join("state.json"))?;
        anyhow::ensure!(
            record.ticket == state.ticket
                && record.base_commit.len() >= 12
                && record.base_commit.is_ascii(),
            "Integration identity changed; preserved {}",
            entry.path().display()
        );
        let suffix = record.base_commit[..12].to_owned();
        records.push((
            record,
            format!("{}-integration-{suffix}", key(&state.ticket)),
            format!("packet/integration/{}/{suffix}", key(&state.ticket)),
        ));
    }
    let mut failures = Vec::new();
    for (record, name, branch) in records {
        if let Err(error) = remove_worktree(repo, &record, &name, &branch, &remote_ref, runner) {
            failures.push(format!("{}: {error:#}", record.worktree.display()));
        }
    }
    anyhow::ensure!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

fn remove_worktree(
    repo: &Path,
    record: &Implementation,
    name: &str,
    branch: &str,
    remote_ref: &str,
    runner: &Runner,
) -> anyhow::Result<()> {
    let expected = repo
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Repository has no parent"))?
        .join(".packet-worktrees")
        .join(crate::persistence::project_slug(&repo.canonicalize()?))
        .join(name);
    anyhow::ensure!(
        record.worktree == expected && record.branch == branch,
        "Worktree path or branch differs from Packet's recorded allocation; preserved"
    );
    let path = record
        .worktree
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid worktree path"))?;
    if let Ok(metadata) = fs::symlink_metadata(&record.worktree) {
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "Worktree is a symlink; preserved"
        );
    }
    if record.worktree.try_exists()? {
        anyhow::ensure!(
            record.worktree.canonicalize()?
                == expected.parent().unwrap().canonicalize()?.join(name),
            "Worktree path changed; preserved"
        );
        anyhow::ensure!(
            common(&record.worktree)?.canonicalize()? == common(repo)?.canonicalize()?
                && runner.git(&record.worktree, &["rev-parse", "--show-toplevel"])? == path
                && runner.git(&record.worktree, &["symbolic-ref", "--short", "HEAD"])? == branch,
            "Worktree repository or branch changed; preserved"
        );
        let head = runner.git(&record.worktree, &["rev-parse", "HEAD"])?;
        anyhow::ensure!(
            record.verified_head.as_deref() == Some(head.as_str()),
            "Worktree has an unverified or changed HEAD; preserved"
        );
        anyhow::ensure!(
            runner
                .git(
                    &record.worktree,
                    &["status", "--porcelain", "--untracked-files=all"]
                )?
                .is_empty(),
            "Worktree contains local changes or untracked files; review them before cleanup can retry"
        );
        // Keep the verified commit reachable even for squash merges whose task
        // branch is not an ancestor of the published commit.
        runner.git(
            repo,
            &[
                "update-ref",
                &format!("refs/packet-evidence/{}/{head}", key(&record.ticket)),
                &head,
            ],
        )?;
        // No --force: Git also protects locked worktrees and submodules. Ignored
        // compiler output is removed along with the clean worktree.
        runner.git(repo, &["worktree", "remove", path])?;
    } else {
        // A killed removal may leave Git registration after deleting files.
        let listing = runner.git(repo, &["worktree", "list", "--porcelain"])?;
        if listing
            .lines()
            .any(|line| line == format!("worktree {path}"))
        {
            runner.git(repo, &["worktree", "remove", path])?;
        }
    }
    // Branch retention is intentional for squash merges and any branch Git
    // cannot safely delete. Never force-delete a branch to reclaim build space.
    if runner
        .git(repo, &["merge-base", "--is-ancestor", branch, remote_ref])
        .is_ok()
    {
        let _ = runner.git(repo, &["branch", "-d", branch]);
    }
    Ok(())
}
