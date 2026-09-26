use super::*;

pub(super) fn auto_publish_enabled(gate: Option<&AtomicBool>) -> bool {
    gate.is_none_or(|gate| gate.load(Ordering::SeqCst))
}

pub(super) fn hold_for_review(dir: &Path, state: &mut Implementation) -> anyhow::Result<()> {
    state.auto_merge = false;
    state.merged_commit = None;
    state.status = ImplementationStatus::ReadyToPublish;
    let note = "Verified locally. No remote changes were made; share for review when ready.";
    if !state.detail.contains(note) {
        state.detail.push('\n');
        state.detail.push_str(note);
    }
    save(dir, state)
}

pub(super) fn create_pull_request(
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
) -> anyhow::Result<()> {
    runner.remaining()?;
    runner.update("Publishing the verified implementation and creating its pull request…");
    // Explicit base/head and body file avoid prompts, accidental forks and shell expansion.
    let remote = runner.git(&state.worktree, &["remote", "get-url", "origin"])?;
    let repository = remote_repository(&remote);
    let prs = runner.command(
        &state.worktree,
        &runner.gh,
        &[
            "pr",
            "list",
            "--repo",
            &repository,
            "--head",
            &state.branch,
            "--base",
            &state.base,
            "--state",
            "all",
            "--json",
            "url,state",
        ],
    )?;
    let existing: Vec<serde_json::Value> = serde_json::from_str(&prs)?;
    if let Some(pr) = existing.first() {
        anyhow::ensure!(
            pr["state"] == "OPEN",
            "The existing PR is closed or merged. Review it before publishing more changes; no duplicate PR created."
        );
    }
    runner.git(
        &state.worktree,
        &["push", "--set-upstream", "origin", &state.branch],
    )?;
    if let Some(pr) = existing.first() {
        anyhow::ensure!(
            pr["state"] != "CLOSED",
            "The existing PR is closed. Review it before continuing; no duplicate PR created."
        );
        state.pr_url = pr["url"].as_str().map(String::from);
    } else {
        let body = dir.join("pr-body.md");
        let url = runner.command(
            &state.worktree,
            &runner.gh,
            &[
                "pr",
                "create",
                "--repo",
                &repository,
                "--head",
                &state.branch,
                "--base",
                &state.base,
                "--title",
                &title(&state.ticket_text),
                "--body-file",
                body.to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid PR body path"))?,
            ],
        )?;
        state.pr_url = Some(url);
    }
    anyhow::ensure!(
        state
            .pr_url
            .as_ref()
            .is_some_and(|url| url.starts_with("https://")),
        "GitHub did not return a PR URL"
    );
    state.status = ImplementationStatus::AwaitingReview;
    state.pr_state = Some(PullRequestState::Open);
    Ok(())
}

pub(super) fn default_branch(repo: &Path, runner: &Runner) -> anyhow::Result<String> {
    let refs = runner.git(repo, &["ls-remote", "--symref", "origin", "HEAD"])?;
    if let Some(branch) = refs.lines().find_map(|line| {
        line.strip_prefix("ref: refs/heads/")
            .and_then(|rest| rest.split_whitespace().next())
    }) {
        return Ok(branch.into());
    }
    let branch = runner.git(repo, &["symbolic-ref", "--short", "HEAD"])?;
    anyhow::ensure!(
        matches!(branch.as_str(), "main" | "master"),
        "Origin has no default branch; configure its HEAD before Auto mode"
    );
    Ok(branch)
}

pub(super) fn finish_auto_publish(
    repo: &Path,
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
) -> anyhow::Result<()> {
    state.status = ImplementationStatus::Completed;
    save(dir, state)?;
    // Remote publication is already durable. Update an idle clean checkout only;
    // a dirty or divergent checkout remains untouched and does not undo success.
    // The writer gate keeps this fast-forward from trampling an in-flight
    // planning checkpoint (turns may commit while workers publish).
    let _guard = crate::core::writer_gate::acquire();
    if runner
        .git(repo, &["status", "--porcelain"])
        .is_ok_and(|status| status.is_empty())
        && runner
            .git(repo, &["symbolic-ref", "--short", "HEAD"])
            .is_ok_and(|branch| branch == state.base)
        && let Some(commit) = &state.merged_commit
        && let Err(error) = runner.git(repo, &["merge", "--ff-only", commit])
    {
        runner.update(format!(
            "Task merged remotely; local checkout could not fast-forward: {error}"
        ));
    }
    Ok(())
}

pub(super) fn remote_repository(remote: &str) -> String {
    let remote = remote.trim().trim_end_matches('/').trim_end_matches(".git");
    if let Some(path) = remote.strip_prefix("git@") {
        return path.replacen(':', "/", 1);
    }
    for scheme in ["https://", "http://", "ssh://"] {
        if let Some(path) = remote.strip_prefix(scheme) {
            return path
                .rsplit_once('@')
                .map(|(_, p)| p)
                .unwrap_or(path)
                .to_owned();
        }
    }
    remote.to_owned()
}

pub(super) fn pr_body(state: &Implementation, report: &Report) -> String {
    let mut text = format!(
        "{}\n\nTicket: `{}`\n\n## Acceptance criteria\n\n",
        report.summary, state.ticket
    );
    for c in &report.acceptance_criteria {
        text.push_str(&format!("- {}: {}\n", c.criterion, c.evidence));
    }
    text.push_str("\n## Validation\n\nPacket reran these commands successfully in the implementation worktree:\n\n");
    for c in &report.verification {
        text.push_str(&format!("```sh\n{c}\n```\n\n"));
    }
    text
}
