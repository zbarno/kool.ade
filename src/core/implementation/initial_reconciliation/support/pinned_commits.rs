use super::*;

pub(in crate::core::implementation) fn pin_plan_commits(
    repo: &Path,
    runner: &Runner,
    task_key: &str,
    local: &str,
    remote: &str,
) -> anyhow::Result<()> {
    for (side, commit) in [("local", local), ("remote", remote)] {
        runner.git(
            repo,
            &[
                "update-ref",
                &format!("refs/koolade-reconciliations/{task_key}/{side}"),
                commit,
            ],
        )?;
    }
    Ok(())
}

pub(in crate::core::implementation) fn validate_pinned_commits(
    repo: &Path,
    runner: &Runner,
    task_key: &str,
    plan: &Plan,
) -> anyhow::Result<()> {
    let pinned = [
        ("local", plan.local_commit.as_str()),
        ("remote", plan.remote_commit.as_str()),
    ];
    for (_, expected) in pinned {
        runner.git(repo, &["cat-file", "-e", &format!("{expected}^{{commit}}")])?;
    }
    runner.git(
        repo,
        &[
            "cat-file",
            "-e",
            &format!("{}^{{commit}}", plan.common_base),
        ],
    )?;
    anyhow::ensure!(
        runner
            .merge_base(repo, &plan.local_commit, &plan.remote_commit)?
            .as_deref()
            == Some(plan.common_base.as_str()),
        "Saved reconciliation common base no longer matches its pinned histories"
    );
    for (side, expected) in pinned {
        let reference = format!("refs/koolade-reconciliations/{task_key}/{side}");
        if runner
            .git(repo, &["update-ref", &reference, expected, expected])
            .is_err()
        {
            let missing = "0".repeat(expected.len());
            runner
                .git(repo, &["update-ref", &reference, expected, &missing])
                .map_err(|error| {
                    anyhow::anyhow!(
                        "The saved reconciliation history reference changed and could not be restored safely: {error:#}"
                    )
                })?;
        }
        let actual = runner.git(repo, &["rev-parse", "--verify", &reference])?;
        anyhow::ensure!(
            actual == expected,
            "Saved reconciliation commit pin mismatch"
        );
    }
    Ok(())
}
