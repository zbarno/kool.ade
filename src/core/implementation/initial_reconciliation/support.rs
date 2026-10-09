pub(in crate::core::implementation) mod generated;
mod pinned_commits;
pub(super) mod quality_checks;
mod verification;

use super::*;
pub(in crate::core::implementation) use pinned_commits::{
    pin_plan_commits, validate_pinned_commits,
};
pub(super) use verification::verification_needs_environment;

pub(super) fn required_baseline_checks(
    worktree: &Path,
    changed: &str,
) -> anyhow::Result<Vec<String>> {
    quality_checks::required_baseline_checks(worktree, changed)
}

pub(in crate::core::implementation) fn required_baseline_checks_for_commits(
    repo: &Path,
    runner: &Runner,
    common_base: &str,
    local: &str,
    remote: &str,
) -> anyhow::Result<Vec<String>> {
    let mut changed_paths = std::collections::BTreeSet::new();
    for commit in [local, remote] {
        let changed = runner.git(
            repo,
            &[
                "diff",
                "--no-renames",
                "--name-only",
                "-z",
                common_base,
                commit,
            ],
        )?;
        changed_paths.extend(
            changed
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(str::to_owned),
        );
    }

    let mut instruction_paths = std::collections::BTreeSet::from([PathBuf::from("AGENTS.md")]);
    for path in &changed_paths {
        let mut directory = Path::new(&path).parent().unwrap_or_else(|| Path::new(""));
        loop {
            instruction_paths.insert(directory.join("AGENTS.md"));
            if directory.as_os_str().is_empty() {
                break;
            }
            directory = directory.parent().unwrap_or_else(|| Path::new(""));
        }
    }

    let mut checks = Vec::new();
    for commit in [local, remote] {
        for path in &instruction_paths {
            let path = path.to_string_lossy();
            if runner
                .git(repo, &["ls-tree", "-z", commit, "--", path.as_ref()])?
                .is_empty()
            {
                continue;
            }
            let object = format!("{commit}:{path}");
            let contents = runner.git(repo, &["show", &object])?;
            let instruction_directory = Path::new(path.as_ref())
                .parent()
                .unwrap_or_else(|| Path::new(""));
            checks.extend(quality_checks::required_commands_in_markdown_for_changes(
                &contents,
                instruction_directory,
                &changed_paths.iter().map(PathBuf::from).collect::<Vec<_>>(),
            )?);
        }
    }
    let mut unique = Vec::new();
    for check in checks {
        if !unique.contains(&check) {
            unique.push(check);
        }
    }
    Ok(unique)
}

pub(super) fn change_sets_are_disjoint(
    repo: &Path,
    runner: &Runner,
    plan: &super::Plan,
) -> anyhow::Result<bool> {
    let local = changed_paths(repo, runner, &plan.common_base, &plan.local_commit)?;
    let remote = changed_paths(repo, runner, &plan.common_base, &plan.remote_commit)?;
    Ok(local.is_disjoint(&remote))
}

pub(super) fn verify_disjoint_changes_preserved(
    worktree: &Path,
    runner: &Runner,
    plan: &super::Plan,
) -> anyhow::Result<()> {
    let local = changed_paths(worktree, runner, &plan.common_base, &plan.local_commit)?;
    let remote = changed_paths(worktree, runner, &plan.common_base, &plan.remote_commit)?;
    anyhow::ensure!(
        local.is_disjoint(&remote),
        "Cannot auto-verify overlapping change sets"
    );
    for (commit, paths) in [(&plan.local_commit, local), (&plan.remote_commit, remote)] {
        for path in paths {
            runner.git(
                worktree,
                &["diff", "--cached", "--quiet", commit, "--", &path],
            )?;
        }
    }
    Ok(())
}

pub(super) fn changed_paths(
    repo: &Path,
    runner: &Runner,
    common_base: &str,
    commit: &str,
) -> anyhow::Result<std::collections::BTreeSet<String>> {
    let output = runner.git(
        repo,
        &[
            "diff",
            "--no-renames",
            "--name-only",
            "-z",
            common_base,
            commit,
        ],
    )?;
    Ok(output
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect())
}

pub(super) fn run_verification(
    runner: &Runner,
    state: &Implementation,
    dir: &Path,
    stamp: i64,
    commands: &[String],
) -> anyhow::Result<VerificationEvidence> {
    let mut results = Vec::new();
    let mut failure = None;
    for command in commands {
        let before = generated::before(runner, state, dir)?;
        let result = runner.verify(&state.task_repository, command);
        let artifacts = generated::after(runner, state, dir, before);
        let (output, error) = match result {
            Ok(output) => (Some(crate::error::redact_secrets(&output)), None),
            Err(error) => {
                let error = crate::error::redact_secrets(&format!("{error:#}"));
                failure.get_or_insert_with(|| format!("`{command}` failed:\n{error}"));
                (None, Some(error))
            }
        };
        results.push(serde_json::json!({"command":command,"output":output,"error":error}));
        if let Err(error) = artifacts {
            failure.get_or_insert_with(|| {
                format!("Could not record verification artifacts: {error:#}")
            });
        }
        if failure.is_some() {
            break;
        }
    }
    crate::artifacts::atomic_write_bytes(
        &dir.join(format!("base-reconciliation-{stamp}-verification.json")),
        &serde_json::to_vec_pretty(&results)?,
    )?;
    Ok(VerificationEvidence { error: failure })
}

#[derive(Debug)]
pub(super) struct VerificationEvidence {
    pub(super) error: Option<String>,
}

pub(super) fn unmerged_paths(runner: &Runner, worktree: &Path) -> anyhow::Result<Vec<String>> {
    let output = runner.git(worktree, &["diff", "--name-only", "--diff-filter=U"])?;
    Ok(output.lines().map(str::to_owned).collect())
}

pub(super) fn validate_task_repository(
    state: &Implementation,
    runner: &Runner,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        state.task_repository_kind == TaskRepositoryKind::Clone,
        "Legacy reconciliation repositories must migrate before use"
    );
    crate::core::implementation::task_repository::validate_clone_path(state)?;
    crate::core::implementation::repository_cache::RepositoryCache::verify_task_repository(
        &state.task_repository,
        runner,
    )?;
    anyhow::ensure!(
        runner.git(&state.task_repository, &["symbolic-ref", "--short", "HEAD"])? == state.branch,
        "Reconciliation task repository identity changed; refusing to modify it"
    );
    Ok(())
}

pub(super) fn is_ancestor(
    runner: &Runner,
    repo: &Path,
    ancestor: &str,
    commit: &str,
) -> anyhow::Result<bool> {
    let Some(merge_base) = runner.merge_base(repo, ancestor, commit)? else {
        return Ok(false);
    };
    let ancestor = runner.git(repo, &["rev-parse", &format!("{ancestor}^{{commit}}")])?;
    Ok(merge_base == ancestor)
}

pub(super) fn ensure_combines(
    repo: &Path,
    runner: &Runner,
    plan: &Plan,
    commit: &str,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        is_ancestor(runner, repo, &plan.local_commit, commit)?
            && is_ancestor(runner, repo, &plan.remote_commit, commit)?,
        "The reconciled starting point does not contain both saved histories"
    );
    Ok(())
}

pub(in crate::core::implementation) fn user_action(detail: String) -> anyhow::Error {
    anyhow::Error::new(super::super::status::FailureCause(Failure::new(
        FailureKind::RemoteDiverged,
        RecoveryDisposition::UserAction,
        detail,
    )))
}

pub(super) fn read_plan(path: &Path) -> anyhow::Result<Plan> {
    let plan: Plan = serde_json::from_slice(&fs::read(path)?)?;
    anyhow::ensure!(
        plan.schema_version == 3,
        "Unsupported reconciliation plan version"
    );
    Ok(plan)
}

pub(super) fn write_plan(path: &Path, plan: &Plan) -> anyhow::Result<()> {
    crate::artifacts::atomic_write_bytes(path, &serde_json::to_vec_pretty(plan)?)
}
