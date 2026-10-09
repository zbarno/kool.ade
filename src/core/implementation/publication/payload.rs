use super::*;

pub(in crate::core::implementation) fn push_identity_remote(
    repo: &Path,
    runner: &Runner,
) -> anyhow::Result<String> {
    effective_push_remote(repo, runner)?;
    match runner.git(repo, &["config", "--get-all", "remote.origin.pushurl"]) {
        Ok(configured) => {
            let urls = configured
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>();
            anyhow::ensure!(
                urls.len() == 1,
                "Repository origin must have exactly one configured push URL for publication"
            );
            Ok(urls[0].to_owned())
        }
        Err(_) => runner.git(repo, &["config", "--get", "remote.origin.url"]),
    }
}

pub(in crate::core::implementation) fn effective_push_remote(
    repo: &Path,
    runner: &Runner,
) -> anyhow::Result<String> {
    let effective = runner.git(repo, &["remote", "get-url", "--push", "--all", "origin"])?;
    let urls = effective
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        urls.len() == 1,
        "Repository origin must have exactly one effective push URL for publication"
    );
    Ok(urls[0].to_owned())
}

pub(in crate::core::implementation) fn validate_target_remotes(
    base_remote: &str,
    push_remote: &str,
    effective_push_remote: &str,
) -> anyhow::Result<String> {
    let base_repository = remote_repository(base_remote);
    let push_repository = remote_repository(push_remote);
    let effective_push_repository = remote_repository(effective_push_remote);
    anyhow::ensure!(
        base_repository.eq_ignore_ascii_case(&push_repository)
            && push_repository.eq_ignore_ascii_case(&effective_push_repository),
        "Automatic pull request and independent-check publication require the configured and effective push destinations to identify the PR base repository; configure origin to push directly to that repository"
    );
    Ok(base_repository)
}

pub(in crate::core::implementation) fn remote_repository(remote: &str) -> String {
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

pub(in crate::core::implementation) fn pr_body(state: &Implementation, report: &Report) -> String {
    let mut text = format!(
        "{}\n\nTicket: `{}`\n\n## Acceptance criteria\n\n",
        report.summary, state.ticket
    );
    for c in &report.acceptance_criteria {
        text.push_str(&format!("- {}: {}\n", c.criterion, c.evidence));
    }
    text.push_str("\n## Validation\n\nKool.ad/e reran these commands successfully in the implementation repository:\n\n");
    for c in &report.verification {
        text.push_str(&format!("```sh\n{c}\n```\n\n"));
    }
    text
}
