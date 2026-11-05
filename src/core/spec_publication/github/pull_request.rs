use super::*;
use anyhow::Context;
use std::path::Path;

pub(in crate::core::spec_publication) struct PullRequestTarget<'a> {
    pub(in crate::core::spec_publication) base: &'a str,
    pub(in crate::core::spec_publication) head: &'a str,
    pub(in crate::core::spec_publication) head_sha: &'a str,
    pub(in crate::core::spec_publication) base_ref: &'a str,
    pub(in crate::core::spec_publication) base_sha: &'a str,
}

pub(in crate::core::spec_publication) fn create_pull_request(
    repository_root: &Path,
    gh: &std::ffi::OsStr,
    repository: &str,
    target: PullRequestTarget<'_>,
    body_path: &Path,
) -> anyhow::Result<String> {
    if let Ok(existing) = gh_output(
        gh,
        repository_root,
        &[
            "pr",
            "view",
            target.head,
            "--repo",
            repository,
            "--json",
            "url,baseRefName,baseRefOid,headRefName,headRefOid,headRepository",
        ],
    ) {
        let existing: serde_json::Value = serde_json::from_str(&existing)
            .context("existing publication pull request response is malformed")?;
        let url = existing["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("existing publication pull request has no URL"))?;
        validate_pull_request_url(repository, url, "existing")?;
        anyhow::ensure!(
            existing["baseRefName"].as_str() == Some(target.base)
                && existing["baseRefOid"].as_str() == Some(target.base_sha)
                && existing["headRefName"].as_str() == Some(target.head)
                && existing["headRefOid"].as_str() == Some(target.head_sha)
                && existing["headRepository"]["nameWithOwner"].as_str() == Some(repository),
            "existing publication pull request no longer matches the approved base and head"
        );
        anyhow::ensure!(
            remote_branch_sha(repository_root, target.base_ref)? == target.base_sha,
            "published documentation advanced before pull request reuse; review the new baseline before publishing"
        );
        return Ok(url.to_owned());
    }
    anyhow::ensure!(
        remote_branch_sha(repository_root, target.base_ref)? == target.base_sha,
        "published documentation advanced before pull request creation; review the new baseline before publishing"
    );
    let body_path = body_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("pull request body path is not UTF-8"))?;
    let output = gh_output(
        gh,
        repository_root,
        &[
            "pr",
            "create",
            "--repo",
            repository,
            "--base",
            target.base,
            "--head",
            target.head,
            "--title",
            "docs: publish approved Kool.ad/e specification",
            "--body-file",
            body_path,
        ],
    )?;
    let url = output.trim();
    validate_pull_request_url(repository, url, "GitHub CLI")?;
    if let Err(error) = verify_created_pull_request(repository_root, gh, repository, &target, url) {
        let close = gh_output(
            gh,
            repository_root,
            &[
                "pr",
                "close",
                url,
                "--repo",
                repository,
                "--comment",
                "Closed because the approved published-spec baseline changed during creation.",
            ],
        );
        return match close {
            Ok(_) => Err(error.context("stale publication pull request was closed")),
            Err(close_error) => Err(error.context(format!(
                "stale publication pull request could not be closed: {close_error}"
            ))),
        };
    }
    Ok(url.to_owned())
}

fn verify_created_pull_request(
    repository_root: &Path,
    gh: &std::ffi::OsStr,
    repository: &str,
    target: &PullRequestTarget<'_>,
    url: &str,
) -> anyhow::Result<()> {
    let view = gh_output(
        gh,
        repository_root,
        &[
            "pr",
            "view",
            url,
            "--repo",
            repository,
            "--json",
            "url,baseRefName,baseRefOid,headRefName,headRefOid,headRepository",
        ],
    )?;
    let view: serde_json::Value =
        serde_json::from_str(&view).context("created publication pull request is malformed")?;
    let returned_url = view["url"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("created publication pull request has no URL"))?;
    validate_pull_request_url(repository, returned_url, "created")?;
    anyhow::ensure!(
        returned_url == url
            && view["baseRefName"].as_str() == Some(target.base)
            && view["baseRefOid"].as_str() == Some(target.base_sha)
            && view["headRefName"].as_str() == Some(target.head)
            && view["headRefOid"].as_str() == Some(target.head_sha)
            && view["headRepository"]["nameWithOwner"].as_str() == Some(repository),
        "created publication pull request no longer matches the approved base and head"
    );
    anyhow::ensure!(
        remote_branch_sha(repository_root, target.base_ref)? == target.base_sha,
        "published documentation advanced during pull request creation; review the new baseline before publishing"
    );
    Ok(())
}

fn validate_pull_request_url(repository: &str, url: &str, source: &str) -> anyhow::Result<()> {
    let expected = format!("https://github.com/{repository}/pull/");
    anyhow::ensure!(
        url.starts_with(&expected) && url[expected.len()..].parse::<u64>().is_ok(),
        "{source} pull request URL does not match the target repository"
    );
    Ok(())
}
