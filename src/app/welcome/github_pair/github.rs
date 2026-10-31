use std::path::Path;
use std::time::Duration;

use super::{PairedRepositoryPreview, RepositoryVisibility, same_github_remote};
const GH_TIMEOUT: Duration = Duration::from_secs(120);
const GH_OUTPUT_LIMIT: usize = 512 * 1024;

pub(super) fn repository_visibility(
    owner: &str,
    repository: &str,
) -> anyhow::Result<Option<RepositoryVisibility>> {
    let endpoint = format!("repos/{owner}/{repository}");
    let output = run_gh(&["api", "--hostname", "github.com", "--include", &endpoint])?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let headers = stdout
        .lines()
        .take_while(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let status = headers.iter().find_map(|line| {
        let mut parts = line.split_whitespace();
        (parts.next().is_some_and(|part| part.starts_with("HTTP/")))
            .then(|| parts.next()?.parse::<u16>().ok())
            .flatten()
    });
    if status == Some(404) {
        return Ok(None);
    }
    if !output.success {
        return Err(gh_failure(
            "could not inspect repository visibility",
            &stderr,
            &stdout,
        ));
    }
    let body = stdout
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .or_else(|| stdout.split_once("\n\n").map(|(_, body)| body))
        .unwrap_or(stdout.as_ref());
    let value: serde_json::Value = serde_json::from_str(body.trim()).map_err(|error| {
        anyhow::anyhow!("GitHub returned an unreadable repository response: {error}")
    })?;
    let visibility = value
        .get("visibility")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("GitHub did not return repository visibility."))?;
    RepositoryVisibility::parse(visibility).map(Some)
}

pub(super) fn create_remote_planning_repository(
    preview: &PairedRepositoryPreview,
    remote: &str,
    local_repository: &Path,
) -> anyhow::Result<()> {
    let source = local_repository.to_str().ok_or_else(|| {
        anyhow::anyhow!("the managed planning repository path is not valid UTF-8")
    })?;
    let name = format!("{}/{}", preview.owner, preview.planning_name);
    let args = [
        "repo",
        "create",
        name.as_str(),
        preview.code_visibility.flag(),
        "--source",
        source,
        "--remote",
        "origin",
        "--push",
    ];
    let output = run_gh(&args)?;
    if !output.success {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(gh_failure(
            &format!(
                "could not create the {} planning repository {name}",
                preview.code_visibility.label()
            ),
            &stderr,
            &stdout,
        ));
    }
    let actual_remote = crate::core::gitops::origin_url(local_repository).ok_or_else(|| {
        anyhow::anyhow!("GitHub created the planning repository but did not configure origin")
    })?;
    anyhow::ensure!(
        same_github_remote(&actual_remote, remote),
        "GitHub created the planning repository, but its local origin points to a different repository"
    );
    let actual_visibility = repository_visibility(&preview.owner, &preview.planning_name)?;
    anyhow::ensure!(
        actual_visibility == Some(preview.code_visibility),
        "GitHub created the planning repository with visibility {}, but the code repository is {}; align their visibility before opening the workspace",
        actual_visibility.map_or("unknown", RepositoryVisibility::label),
        preview.code_visibility.label()
    );
    Ok(())
}

fn run_gh(args: &[&str]) -> anyhow::Result<crate::harness::discovery::ProbeOutput> {
    let argv = std::iter::once("gh".to_owned())
        .chain(args.iter().map(|argument| (*argument).to_owned()))
        .collect::<Vec<_>>();
    crate::harness::discovery::run_with_environment(
        &argv,
        GH_TIMEOUT,
        GH_OUTPUT_LIMIT,
        GH_OUTPUT_LIMIT,
        &[],
        &[
            ("GH_HOST", "github.com"),
            ("GH_PROMPT_DISABLED", "1"),
            ("GH_PAGER", "cat"),
            ("NO_COLOR", "1"),
        ],
    )
    .map_err(|error| {
        anyhow::anyhow!(
            "GitHub CLI could not run. Install `gh`, run `gh auth login`, and retry: {error}"
        )
    })
}

fn gh_failure(action: &str, stderr: &str, stdout: &str) -> anyhow::Error {
    let detail = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    let detail = crate::error::redact_secrets(detail);
    anyhow::anyhow!(
        "GitHub CLI {action}. Check `gh auth status` and repository access, then retry. {}",
        if detail.is_empty() {
            String::new()
        } else {
            format!("GitHub said: {detail}")
        }
    )
}
