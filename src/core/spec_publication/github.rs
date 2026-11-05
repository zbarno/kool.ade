use super::git::*;
use std::path::Path;

mod pull_request;
pub(super) use pull_request::{PullRequestTarget, create_pull_request};

pub(super) fn branch_from_reference(reference: &str) -> anyhow::Result<String> {
    let branch = reference
        .strip_prefix("refs/heads/")
        .or_else(|| reference.strip_prefix("refs/remotes/origin/"))
        .ok_or_else(|| anyhow::anyhow!("published ref must identify an origin branch"))?;
    validate_branch(branch)?;
    Ok(branch.to_owned())
}

pub(super) fn validate_branch(branch: &str) -> anyhow::Result<()> {
    let output = git_standalone_output(&["check-ref-format", &format!("refs/heads/{branch}")])?;
    anyhow::ensure!(output.status.success(), "Git branch name is invalid");
    Ok(())
}

pub(super) fn trusted_origin_url(repository: &Path) -> anyhow::Result<String> {
    crate::core::git_commands::validate_repository_config(repository)?;
    let fetch = git_at(repository, &["remote", "get-url", "--all", "origin"])?;
    let push = git_at(
        repository,
        &["remote", "get-url", "--push", "--all", "origin"],
    )?;
    let fetch_urls = fetch.lines().collect::<Vec<_>>();
    let push_urls = push.lines().collect::<Vec<_>>();
    anyhow::ensure!(
        fetch_urls.len() == 1 && push_urls.len() == 1 && fetch_urls[0] == push_urls[0],
        "origin must have one identical fetch and push URL"
    );
    anyhow::ensure!(!fetch_urls[0].trim().is_empty(), "origin URL is empty");
    reject_embedded_remote_credentials(fetch_urls[0])?;
    Ok(fetch_urls[0].to_owned())
}

pub(super) fn reject_embedded_remote_credentials(remote: &str) -> anyhow::Result<()> {
    if let Some((scheme, remainder)) = remote.split_once("://")
        && matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https")
    {
        let authority = remainder.split('/').next().unwrap_or_default();
        anyhow::ensure!(
            !authority.contains('@') && !remainder.contains('?') && !remainder.contains('#'),
            "origin URL must not embed credentials"
        );
    }
    anyhow::ensure!(
        !remote.chars().any(char::is_control),
        "origin URL contains unsupported control characters"
    );
    Ok(())
}

pub(super) fn github_repository_from_remote(remote: &str) -> anyhow::Result<Option<String>> {
    let (host, path) = if let Some((scheme, remainder)) = remote.split_once("://") {
        if !matches!(scheme.to_ascii_lowercase().as_str(), "https" | "ssh") {
            return Ok(None);
        }
        let (authority, path) = remainder
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("origin URL does not include a repository path"))?;
        let host = authority.rsplit('@').next().unwrap_or_default();
        (host, path)
    } else if let Some((authority, path)) = remote.split_once(':') {
        let host = authority.rsplit('@').next().unwrap_or_default();
        (host, path)
    } else {
        return Ok(None);
    };
    if !host.eq_ignore_ascii_case("github.com") {
        return Ok(None);
    }
    let trimmed = path.trim_end_matches('/');
    let path = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    let parts = path.split('/').collect::<Vec<_>>();
    anyhow::ensure!(
        parts.len() == 2
            && !parts[0].is_empty()
            && parts[0].len() <= 39
            && parts[0]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !parts[1].is_empty()
            && parts[1].len() <= 100
            && parts[1]
                .bytes()
                .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') }),
        "origin URL does not identify a GitHub owner and repository"
    );
    Ok(Some(format!("{}/{}", parts[0], parts[1])))
}

pub(super) fn remote_branch_sha(repository: &Path, reference: &str) -> anyhow::Result<String> {
    let output = git_at(
        repository,
        &["ls-remote", "--exit-code", "--heads", "origin", reference],
    )?;
    let (sha, found_ref) = output
        .split_once('\t')
        .ok_or_else(|| anyhow::anyhow!("published branch was not found on origin"))?;
    anyhow::ensure!(
        found_ref.trim() == reference && is_object_id(sha.trim()),
        "origin returned an invalid published branch identity"
    );
    Ok(sha.trim().to_owned())
}

pub(super) fn remote_branch_sha_optional(
    repository: &Path,
    reference: &str,
) -> anyhow::Result<Option<String>> {
    let output = git_at(repository, &["ls-remote", "--heads", "origin", reference])?;
    if output.is_empty() {
        return Ok(None);
    }
    let (sha, found_ref) = output
        .split_once('\t')
        .ok_or_else(|| anyhow::anyhow!("origin returned a malformed publication branch"))?;
    anyhow::ensure!(
        found_ref.trim() == reference && is_object_id(sha.trim()),
        "origin returned an invalid publication branch identity"
    );
    Ok(Some(sha.trim().to_owned()))
}

pub(super) fn github_repository(repository: &Path, gh: &std::ffi::OsStr) -> anyhow::Result<String> {
    let output = gh_output(
        gh,
        repository,
        &[
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "--jq",
            ".nameWithOwner",
        ],
    )?;
    let name = output.trim();
    let parts = name.split('/').collect::<Vec<_>>();
    anyhow::ensure!(
        parts.len() == 2
            && !parts[0].is_empty()
            && parts[0].len() <= 39
            && parts[0]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && !parts[1].is_empty()
            && parts[1].len() <= 100
            && parts[1]
                .bytes()
                .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') }),
        "GitHub did not return a valid repository identity"
    );
    Ok(name.to_owned())
}

pub(super) struct PublicCommitIdentity {
    pub(super) name: String,
    pub(super) email: String,
}

pub(super) fn public_commit_identity(
    gh: &std::ffi::OsStr,
    repository: &Path,
) -> anyhow::Result<PublicCommitIdentity> {
    let identity = gh_output(
        gh,
        repository,
        &["api", "user", "--jq", "[.id, .login] | @tsv"],
    )?;
    let (id, login) = identity
        .split_once('\t')
        .ok_or_else(|| anyhow::anyhow!("GitHub identity response is malformed"))?;
    anyhow::ensure!(
        id.len() <= 20
            && !id.is_empty()
            && id.bytes().all(|byte| byte.is_ascii_digit())
            && !login.is_empty()
            && login.len() <= 39
            && login
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "GitHub identity response is invalid"
    );
    Ok(PublicCommitIdentity {
        name: login.to_owned(),
        email: format!("{id}+{login}@users.noreply.github.com"),
    })
}
