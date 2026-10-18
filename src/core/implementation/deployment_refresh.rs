use super::{Runner, task::ticket_identity};
use crate::{
    artifacts::planning_store::PlanningStore,
    core::lifecycle::{DeploymentObservation, LifecycleStore},
};
use anyhow::{Context, bail};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct Deployment {
    id: u64,
    sha: String,
    environment: String,
    #[serde(default)]
    production_environment: bool,
    created_at: String,
}

#[derive(Deserialize)]
struct Status {
    id: u64,
    state: String,
    environment_url: Option<String>,
    created_at: String,
}

pub(super) fn refresh(
    store: &PlanningStore,
    state_root: &Path,
    ticket: &str,
    runner: &Runner,
) -> anyhow::Result<()> {
    let Some(task_uid) = ticket_identity(store, ticket)? else {
        return Ok(());
    };
    let lifecycle = LifecycleStore::new(store);
    let Some(task) = lifecycle
        .list_tasks()?
        .into_iter()
        .find(|task| task.task_uid == task_uid)
    else {
        return Ok(());
    };
    let Some(current) = lifecycle.load_current_implementation(&task.feature_uid)? else {
        return Ok(());
    };
    if current.task_uid != task_uid {
        return Ok(());
    }
    let work = lifecycle.load_pr_preparation(&current.work_item_uid)?;
    let Some(pr_url) = work.provider_pr_url.as_deref() else {
        return Ok(());
    };
    let Some(pr_id) = work.provider_pr_id else {
        bail!("Persisted PR preparation has no provider PR ID");
    };
    let target_repo = super::target_repository_with_store(store, state_root, ticket)?;
    lifecycle
        .validate_deployment_repository(&current, &target_repo, pr_url, pr_id)
        .map_err(|error| anyhow::anyhow!("Deployment repository binding is invalid: {error}"))?;
    let repository = github_repository_from_pr_url(pr_url)?;
    let deployments_path = format!(
        "repos/{repository}/deployments?sha={}&per_page=100",
        current.merge_commit_sha
    );
    let output = runner.command(state_root, &runner.gh, &["api", &deployments_path])?;
    let deployments: Vec<Deployment> =
        serde_json::from_str(&output).context("GitHub returned an invalid deployment list")?;
    let observation = latest_production_observation(
        deployments,
        &current.merge_commit_sha,
        &repository,
        state_root,
        runner,
    )?;
    lifecycle.record_deployment_evidence(&current, observation)?;
    Ok(())
}

pub(crate) fn needs_refresh(store: &PlanningStore, ticket: &str) -> anyhow::Result<bool> {
    let Some(task_uid) = ticket_identity(store, ticket)? else {
        return Ok(false);
    };
    let lifecycle = LifecycleStore::new(store);
    let Some(task) = lifecycle
        .list_tasks()?
        .into_iter()
        .find(|task| task.task_uid == task_uid)
    else {
        return Ok(false);
    };
    let Some(current) = lifecycle.load_current_implementation(&task.feature_uid)? else {
        return Ok(true);
    };
    let Some(deployment) = lifecycle.load_deployment_evidence(&task.feature_uid)? else {
        return Ok(true);
    };
    Ok(deployment.deployed_spec_hash(&current).is_none())
}

fn latest_production_observation(
    deployments: Vec<Deployment>,
    merge_sha: &str,
    repository: &str,
    state_root: &Path,
    runner: &Runner,
) -> anyhow::Result<DeploymentObservation> {
    let Some(deployment) = latest_matching_production_deployment(deployments, merge_sha) else {
        return Ok(DeploymentObservation::default());
    };
    let statuses_path = format!(
        "repos/{}/deployments/{}/statuses?per_page=100",
        repository, deployment.id
    );
    let output = runner.command(state_root, &runner.gh, &["api", &statuses_path])?;
    let statuses: Vec<Status> =
        serde_json::from_str(&output).context("GitHub returned invalid deployment statuses")?;
    let Some(status) = latest_status(statuses) else {
        return Ok(DeploymentObservation {
            deployment_id: Some(deployment.id),
            environment: Some(deployment.environment),
            production_environment: true,
            ..DeploymentObservation::default()
        });
    };
    let _status_environment_url = status.environment_url;
    Ok(DeploymentObservation {
        deployment_id: Some(deployment.id),
        status_id: Some(status.id),
        environment: Some(deployment.environment),
        production_environment: true,
        state: Some(status.state),
    })
}

fn latest_matching_production_deployment(
    deployments: Vec<Deployment>,
    merge_sha: &str,
) -> Option<Deployment> {
    let mut candidates = deployments
        .into_iter()
        .filter(|deployment| {
            deployment.production_environment
                && deployment.sha.eq_ignore_ascii_case(merge_sha)
                && !deployment.environment.trim().is_empty()
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    candidates.into_iter().next()
}

fn latest_status(mut statuses: Vec<Status>) -> Option<Status> {
    statuses.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    statuses.into_iter().next()
}

fn github_repository_from_pr_url(url: &str) -> anyhow::Result<String> {
    let url = url::Url::parse(url).context("Persisted PR URL is invalid")?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") || url.port().is_some() {
        bail!("Persisted PR URL is not a GitHub HTTPS URL");
    }
    let segments = url
        .path_segments()
        .ok_or_else(|| anyhow::anyhow!("Persisted PR URL has no path"))?
        .collect::<Vec<_>>();
    if segments.len() != 4
        || segments[2] != "pull"
        || segments[0].is_empty()
        || segments[1].is_empty()
        || segments[3].parse::<u64>().is_err()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        bail!("Persisted PR URL has an unexpected path");
    }
    let valid_component = |value: &str| {
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
    };
    if !valid_component(segments[0]) || !valid_component(segments[1]) {
        bail!("Persisted PR URL repository name is malformed");
    }
    Ok(format!("{}/{}", segments[0], segments[1]))
}

#[cfg(test)]
mod tests {
    use super::{
        Deployment, Status, github_repository_from_pr_url, latest_matching_production_deployment,
        latest_status,
    };

    /// Synthetic GitHub API fixtures cover the provider-field boundary without network access.
    /// This is a parser/selection unit test, not a deployment E2E test.
    /// The production boundary is GitHub's read-only Deployments and statuses API; this fixture
    /// test proves current-SHA, production-environment, and latest-status selection invariants.
    /// It has no external dependencies and negligible setup cost.
    #[test]
    fn deployment_api_selection_requires_current_sha_and_production_environment() {
        let deployments: Vec<Deployment> = serde_json::from_str(
            r#"[
                {"id":1,"sha":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","environment":"production","production_environment":true,"created_at":"2026-10-17T10:00:00Z"},
                {"id":2,"sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","environment":"production","production_environment":true,"created_at":"2026-10-18T10:00:00Z"},
                {"id":3,"sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","environment":"preview","production_environment":false,"created_at":"2026-10-18T11:00:00Z"}
            ]"#,
        )
        .unwrap();
        assert_eq!(
            latest_matching_production_deployment(deployments, &"b".repeat(40))
                .map(|deployment| deployment.id),
            Some(2)
        );
        assert!(latest_matching_production_deployment(Vec::new(), &"c".repeat(40)).is_none());
    }

    #[test]
    fn deployment_api_uses_latest_status_instead_of_an_older_success() {
        let statuses: Vec<Status> = serde_json::from_str(
            r#"[
                {"id":8,"state":"success","created_at":"2026-10-17T10:00:00Z"},
                {"id":9,"state":"in_progress","created_at":"2026-10-18T10:00:00Z"}
            ]"#,
        )
        .unwrap();
        let latest = latest_status(statuses).unwrap();
        assert_eq!(latest.id, 9);
        assert_eq!(latest.state, "in_progress");
    }

    #[test]
    fn github_repository_requires_a_canonical_pull_request_url() {
        assert_eq!(
            github_repository_from_pr_url("https://github.com/owner/repo/pull/42").unwrap(),
            "owner/repo"
        );
        for invalid in [
            "http://github.com/owner/repo/pull/42",
            "https://github.com.evil.test/owner/repo/pull/42",
            "https://github.com/owner/repo/issues/42",
            "https://github.com/owner/repo/pull/not-a-number",
        ] {
            assert!(github_repository_from_pr_url(invalid).is_err(), "{invalid}");
        }
    }
}
