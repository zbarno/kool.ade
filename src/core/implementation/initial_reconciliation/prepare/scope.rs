use super::*;
use std::collections::BTreeSet;

pub(super) fn ensure_pinned_path_scope(
    repo: &Path,
    worktree: &Path,
    runner: &Runner,
    plan: &Plan,
) -> anyhow::Result<()> {
    let head = runner.git(worktree, &["rev-parse", "HEAD"])?;
    let merge_head = super::auto_verify::current_merge_head(runner, worktree)?;
    if head == plan.remote_commit && merge_head.as_deref() == Some(plan.local_commit.as_str()) {
        // Let the reconciliation guard classify and snapshot unexpected state
        // from this exact pinned merge before the general path-scope gate runs.
        return Ok(());
    }
    let mut allowed = BTreeSet::new();
    for commit in [&plan.local_commit, &plan.remote_commit] {
        allowed.extend(changed_paths(repo, runner, &plan.common_base, commit)?);
    }

    let mut observed = BTreeSet::new();
    for args in [
        vec!["diff", "--cached", "--name-only", "--no-renames", "-z"],
        vec!["diff", "--name-only", "--no-renames", "-z"],
        vec!["ls-files", "--others", "--exclude-standard", "-z"],
    ] {
        let output = runner.git(worktree, &args)?;
        observed.extend(
            output
                .split('\0')
                .filter(|path| !path.is_empty())
                .map(str::to_owned),
        );
    }

    let unexpected = unexpected_paths(&allowed, &observed);
    if unexpected.is_empty() {
        return Ok(());
    }
    let listed = unexpected.iter().take(20).cloned().collect::<Vec<_>>();
    let suffix = if unexpected.len() > listed.len() {
        format!(" and {} more", unexpected.len() - listed.len())
    } else {
        String::new()
    };
    Err(super::super::support::user_action(format!(
        "Reconciliation stopped before further agent work: the isolated worktree contains changes outside the two pinned histories: {}{suffix}. These paths are preserved. Review or remove the unrelated changes, then resume.",
        listed.join(", ")
    )))
}

fn unexpected_paths(allowed: &BTreeSet<String>, observed: &BTreeSet<String>) -> Vec<String> {
    observed.difference(allowed).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_paths_outside_the_pinned_histories_without_adopting_them() {
        let allowed = BTreeSet::from(["Source/Startup.cs".to_owned()]);
        let observed = BTreeSet::from([
            "Source/Startup.cs".to_owned(),
            "Source/Directory.Build.props".to_owned(),
        ]);

        let unexpected = unexpected_paths(&allowed, &observed);

        assert_eq!(unexpected, ["Source/Directory.Build.props"]);
    }
}
