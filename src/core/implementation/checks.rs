//! Independent provider checks, separate from Packet's local verification.
use super::{IndependentCheckStatus, Runner};
use anyhow::Context;
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResultState {
    Pending,
    Passed,
    Failed(String),
}

pub(super) trait Provider {
    fn name(&self) -> &'static str;
    fn repository(&self, remote: &str) -> Option<String>;
    fn check(
        &self,
        runner: &Runner,
        cwd: &Path,
        repository: &str,
        commit: &str,
    ) -> anyhow::Result<ResultState>;
}

pub(super) struct GitHubActions;

impl Provider for GitHubActions {
    fn name(&self) -> &'static str {
        "GitHub Actions"
    }

    fn repository(&self, remote: &str) -> Option<String> {
        let remote = remote.trim().trim_end_matches('/').trim_end_matches(".git");
        let path = if let Some(path) = remote.strip_prefix("git@github.com:") {
            path
        } else if let Some(path) = remote.strip_prefix("ssh://git@github.com/") {
            path
        } else {
            github_path(
                remote
                    .strip_prefix("https://")
                    .or_else(|| remote.strip_prefix("http://"))?,
            )?
        };
        let mut parts = path.split('/');
        let (owner, name) = (parts.next()?, parts.next()?);
        if owner.is_empty() || name.is_empty() || parts.next().is_some() {
            return None;
        }
        Some(format!("github.com/{owner}/{name}"))
    }

    fn check(
        &self,
        runner: &Runner,
        cwd: &Path,
        repository: &str,
        commit: &str,
    ) -> anyhow::Result<ResultState> {
        let output = runner.command(
            cwd,
            &runner.gh,
            &[
                "run",
                "list",
                "--repo",
                repository,
                "--commit",
                commit,
                "--json",
                "headSha,status,conclusion,workflowName,createdAt,url",
                "--limit",
                "100",
            ],
        )?;
        parse_runs(&output, commit)
    }
}

fn github_path(host_and_path: &str) -> Option<&str> {
    let host_and_path = host_and_path
        .rsplit_once('@')
        .map_or(host_and_path, |(_, rest)| rest);
    host_and_path.strip_prefix("github.com/")
}

pub(super) fn for_remote(remote: &str) -> Option<&'static dyn Provider> {
    GitHubActions
        .repository(remote)
        .map(|_| &GitHubActions as &dyn Provider)
}

pub(super) fn candidate_ref(ticket_key: &str, commit: &str) -> String {
    format!("refs/heads/packet/checks/{ticket_key}/{commit}")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Run {
    #[serde(default)]
    head_sha: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    workflow_name: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    url: String,
}

fn parse_runs(output: &str, commit: &str) -> anyhow::Result<ResultState> {
    let runs: Vec<Run> =
        serde_json::from_str(output).context("GitHub returned invalid workflow-check data")?;
    let mut latest = BTreeMap::<String, Run>::new();
    for run in runs.into_iter().filter(|run| run.head_sha == commit) {
        let key = if run.workflow_name.is_empty() {
            run.url.clone()
        } else {
            run.workflow_name.clone()
        };
        if latest
            .get(&key)
            .is_none_or(|saved| run.created_at > saved.created_at)
        {
            latest.insert(key, run);
        }
    }
    if latest.is_empty() || latest.values().any(|run| run.status != "completed") {
        return Ok(ResultState::Pending);
    }
    let failures = latest
        .values()
        .filter(|run| run.conclusion.as_deref() != Some("success"))
        .map(|run| {
            format!(
                "{} concluded {}",
                if run.workflow_name.is_empty() {
                    "A workflow"
                } else {
                    &run.workflow_name
                },
                run.conclusion.as_deref().unwrap_or("without a result")
            )
        })
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(ResultState::Passed)
    } else {
        Ok(ResultState::Failed(failures.join("; ")))
    }
}

pub(super) fn persisted_status(result: &ResultState) -> IndependentCheckStatus {
    match result {
        ResultState::Pending => IndependentCheckStatus::Pending,
        ResultState::Passed => IndependentCheckStatus::Passed,
        ResultState::Failed(_) => IndependentCheckStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_github_remotes_are_supported_and_candidate_refs_are_commit_specific() {
        let provider = GitHubActions;
        assert_eq!(
            provider.repository("git@github.com:owner/project.git"),
            Some("github.com/owner/project".into())
        );
        assert_eq!(
            provider.repository("https://github.com/owner/project"),
            Some("github.com/owner/project".into())
        );
        assert_eq!(
            provider.repository("https://token:secret@github.com/owner/project.git"),
            Some("github.com/owner/project".into())
        );
        assert_eq!(
            provider.repository("https://gitlab.com/owner/project"),
            None
        );
        assert_eq!(
            candidate_ref("task-1", "abc123"),
            "refs/heads/packet/checks/task-1/abc123"
        );
    }

    #[test]
    fn exact_commit_runs_must_all_finish_successfully() {
        let passing = r#"[
          {"headSha":"abc","status":"completed","conclusion":"success","workflowName":"CI","createdAt":"2026-01-02T00:00:00Z","url":"https://example/1"},
          {"headSha":"other","status":"completed","conclusion":"failure","workflowName":"Old","createdAt":"2026-01-02T00:00:00Z","url":"https://example/2"}
        ]"#;
        assert_eq!(parse_runs(passing, "abc").unwrap(), ResultState::Passed);

        let pending = r#"[{"headSha":"abc","status":"in_progress","workflowName":"CI"}]"#;
        assert_eq!(parse_runs(pending, "abc").unwrap(), ResultState::Pending);
        assert_eq!(parse_runs("[]", "abc").unwrap(), ResultState::Pending);

        let failed = r#"[{"headSha":"abc","status":"completed","conclusion":"failure","workflowName":"CI"}]"#;
        assert!(matches!(
            parse_runs(failed, "abc").unwrap(),
            ResultState::Failed(_)
        ));
    }

    #[test]
    fn successful_rerun_supersedes_older_failure_for_same_workflow() {
        let reruns = r#"[
          {"headSha":"abc","status":"completed","conclusion":"failure","workflowName":"CI","createdAt":"2026-01-01T00:00:00Z"},
          {"headSha":"abc","status":"completed","conclusion":"success","workflowName":"CI","createdAt":"2026-01-02T00:00:00Z"}
        ]"#;
        assert_eq!(parse_runs(reruns, "abc").unwrap(), ResultState::Passed);
    }
}
