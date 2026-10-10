use super::*;

fn git(repo: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
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
        if git(
            &repository,
            &["cat-file", "-e", &format!("{merged}^{{commit}}")],
        )
        .is_err()
        {
            git(&repository, &["fetch", "--no-tags", "origin"])?;
        }
        git(
            &repository,
            &["cat-file", "-e", &format!("{merged}^{{commit}}")],
        )?;
        git(
            &repository,
            &["merge-base", "--is-ancestor", &task.base_commit, merged],
        )?;
        let paths = git(
            &repository,
            &["diff", "--name-only", &task.base_commit, merged],
        )?;
        text.push_str(&format!(
            "\nTask: {}\nTarget checkout: {}\nBase: {}\nMerged: {}\nChanged paths:\n{}\n",
            task.ticket,
            repository.display(),
            task.base_commit,
            merged,
            paths.lines().take(150).collect::<Vec<_>>().join("\n")
        ));
    }
    Ok(text)
}
