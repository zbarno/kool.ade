use super::*;

const HOLD_FOR_REVIEW_NOTE: &str =
    "Verified locally. No remote changes were made; share for review when ready.";
const PR_STATUS_PREFIX: &str = "Pull request status: ";

pub(super) fn auto_publish_enabled(gate: Option<&AtomicBool>) -> bool {
    gate.is_none_or(|gate| gate.load(Ordering::SeqCst))
}

pub(super) fn hold_for_review(dir: &Path, state: &mut Implementation) -> anyhow::Result<()> {
    state.auto_merge = false;
    state.merged_commit = None;
    state.status = ImplementationStatus::ReadyToPublish;
    update_pull_request_detail(state);
    if !state
        .detail
        .lines()
        .any(|line| line == HOLD_FOR_REVIEW_NOTE)
    {
        if !state.detail.is_empty() {
            state.detail.push('\n');
        }
        state.detail.push_str(HOLD_FOR_REVIEW_NOTE);
    }
    save(dir, state)
}

pub(super) fn create_pull_request(
    dir: &Path,
    state: &mut Implementation,
    runner: &Runner,
) -> anyhow::Result<()> {
    runner.remaining()?;
    anyhow::ensure!(
        !matches!(state.branch.as_str(), "main" | "master"),
        "Refusing to push implementation changes from the main or master branch"
    );
    anyhow::ensure!(
        state.branch.starts_with("koolade/"),
        "Refusing to push implementation changes from a non-Kool.ad/e branch"
    );
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
    let current_branch = runner.git(&state.worktree, &["symbolic-ref", "--short", "HEAD"])?;
    anyhow::ensure!(
        current_branch == state.branch,
        "Implementation worktree branch changed; refusing to push"
    );
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
    update_pull_request_detail(state);
    Ok(())
}

pub(super) fn update_pull_request_detail(state: &mut Implementation) {
    state.detail = state
        .detail
        .lines()
        .filter(|line| *line != HOLD_FOR_REVIEW_NOTE && !line.starts_with(PR_STATUS_PREFIX))
        .collect::<Vec<_>>()
        .join("\n");
    let (Some(status), Some(url)) = (state.pr_state, state.pr_url.as_deref()) else {
        return;
    };
    if !state.detail.is_empty() {
        state.detail.push('\n');
    }
    let status = match status {
        PullRequestState::Open => format!("Open: {url}"),
        PullRequestState::Closed => format!("Closed: {url}"),
        PullRequestState::Merged => {
            let commit = state
                .merged_commit
                .as_deref()
                .and_then(|commit| commit.get(..12))
                .map(|commit| format!(" as {commit}"))
                .unwrap_or_default();
            format!("Merged{commit}: {url}")
        }
    };
    state.detail.push_str(PR_STATUS_PREFIX);
    state.detail.push_str(&status);
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
    text.push_str("\n## Validation\n\nKool.ad/e reran these commands successfully in the implementation worktree:\n\n");
    for c in &report.verification {
        text.push_str(&format!("```sh\n{c}\n```\n\n"));
    }
    text
}
