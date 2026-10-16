use super::*;

fn git(repo: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = crate::core::planning_sync::validated_git_command(repo)?
        .args(args)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        crate::error::redact_secrets(&String::from_utf8_lossy(&output.stderr))
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

pub(super) fn implementation_evidence(
    state: &PlannerState,
    candidate: &Candidate,
) -> anyhow::Result<String> {
    let mut text = String::new();
    for task in &candidate.tasks {
        let repository = crate::core::implementation::target_repository_with_store(
            &state.planning_store,
            &state.repo_root,
            &task.ticket,
        )?;
        let merged = task.merged_commit.as_deref().unwrap();
        let pull_request_head = task
            .verified_head
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("Task has no verified pull request head"))?;
        if crate::core::reconciliation::merge_verification::validate_merged_checkout(
            &repository,
            &task.base_commit,
            pull_request_head,
            merged,
        )
        .is_err()
        {
            git(&repository, &["fetch", "--no-tags", "origin"])?;
        }
        let verified = crate::core::reconciliation::merge_verification::validate_merged_checkout(
            &repository,
            &task.base_commit,
            pull_request_head,
            merged,
        )?;
        let paths = git(
            &repository,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--name-only",
                &task.base_commit,
                merged,
            ],
        )?;
        text.push_str(&format!(
            "\nTask: {}\nBase: {}\nPR head: {}\nValidated merge: {}\nChecked out head: {}\nChanged paths:\n{}\n",
            task.ticket,
            task.base_commit,
            verified.pull_request_head,
            verified.merge_commit,
            verified.checked_out_head,
            paths.lines().take(150).collect::<Vec<_>>().join("\n")
        ));
    }
    Ok(text)
}
