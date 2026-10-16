//! Deterministic validation of provider-reported PR merge data against Git.
use std::path::Path;

mod fetch;
#[cfg(test)]
pub(crate) use fetch::run_bounded_process;
pub use fetch::{fetch_origin_with_deadline, fetch_pull_request_head};

/// Evidence that the reported PR head was merged and is present in the
/// repository's checked-out commit graph. This contains no provider prose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedMergeEvidence {
    pub base_commit: String,
    pub pull_request_head: String,
    pub merge_commit: String,
    pub checked_out_head: String,
}

/// Validate exact Git objects and ancestry before any current-implementation
/// projection can consume provider merge metadata.
pub fn validate_merged_checkout(
    repository: &Path,
    base_commit: &str,
    pull_request_head: &str,
    reported_merge_commit: &str,
) -> anyhow::Result<ValidatedMergeEvidence> {
    anyhow::ensure!(
        valid_oid(base_commit) && valid_oid(pull_request_head) && valid_oid(reported_merge_commit),
        "PR merge evidence contains an invalid Git object ID"
    );
    let head = git(repository, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let merge = git(
        repository,
        &[
            "rev-parse",
            "--verify",
            &format!("{reported_merge_commit}^{{commit}}"),
        ],
    )?;
    let pr_head = git(
        repository,
        &[
            "rev-parse",
            "--verify",
            &format!("{pull_request_head}^{{commit}}"),
        ],
    )?;
    anyhow::ensure!(
        merge == reported_merge_commit && pr_head == pull_request_head,
        "provider-reported PR merge or head SHA does not resolve to the exact Git object"
    );
    let target_parent = git(
        repository,
        &[
            "rev-parse",
            "--verify",
            &format!("{reported_merge_commit}^1"),
        ],
    )?;
    anyhow::ensure!(
        is_ancestor(repository, base_commit, &target_parent)?,
        "recorded provider PR base is not an ancestor of the merged target"
    );
    let source_is_ancestor = is_ancestor(repository, pull_request_head, reported_merge_commit)?;
    anyhow::ensure!(
        source_is_ancestor
            || same_patch(
                repository,
                &target_parent,
                pull_request_head,
                reported_merge_commit
            )?,
        "PR head changes are not present in the reported merge commit"
    );
    anyhow::ensure!(
        is_ancestor(repository, reported_merge_commit, &head)?,
        "reported merge commit is not present in the checked-out repository history"
    );
    Ok(ValidatedMergeEvidence {
        base_commit: base_commit.to_owned(),
        pull_request_head: pr_head,
        merge_commit: merge,
        checked_out_head: head,
    })
}

/// Bind the origin used for fetching the provider PR ref to both the persisted
/// repository identity and the HTTPS repository encoded in the PR URL.
pub fn validate_pr_repository_binding(
    repository_id: &str,
    provider_pr_url: &str,
    pull_request_number: u64,
    registered_repository_url: &str,
    effective_origin_url: &str,
) -> anyhow::Result<()> {
    let url = url::Url::parse(provider_pr_url)?;
    anyhow::ensure!(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "persisted PR URL is not a public-safe HTTPS provider URL"
    );
    let segments = url
        .path_segments()
        .ok_or_else(|| anyhow::anyhow!("persisted PR URL has no repository path"))?
        .collect::<Vec<_>>();
    anyhow::ensure!(
        segments.len() == 4
            && segments[2] == "pull"
            && segments[3].parse::<u64>().ok() == Some(pull_request_number),
        "persisted PR URL does not match its provider number"
    );
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("PR URL has no host"))?;
    let port = url
        .port()
        .filter(|port| Some(*port) != url.port_or_known_default())
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let expected_repository = format!("{host}{port}/{}/{}", segments[0], segments[1]);
    anyhow::ensure!(
        !repository_id.trim().is_empty() && repository_id.len() <= 80,
        "task repository identity is invalid"
    );
    let registered_identity = crate::core::planning_sync::remote_identity_for(
        std::path::Path::new("."),
        registered_repository_url,
    );
    anyhow::ensure!(
        registered_identity == format!("git://{expected_repository}"),
        "persisted PR URL does not match the registered task repository"
    );
    let origin_identity = crate::core::planning_sync::remote_identity_for(
        std::path::Path::new("."),
        effective_origin_url,
    );
    anyhow::ensure!(
        origin_identity == format!("git://{expected_repository}"),
        "origin used for PR evidence does not match the persisted provider repository"
    );
    Ok(())
}

fn same_patch(
    repository: &Path,
    actual_target_base: &str,
    pr_head: &str,
    merge: &str,
) -> anyhow::Result<bool> {
    let contribution_base = git(repository, &["merge-base", actual_target_base, pr_head])?;
    Ok(patch_id(repository, &contribution_base, pr_head)?
        == patch_id(repository, actual_target_base, merge)?)
}

fn patch_id(repository: &Path, base: &str, head: &str) -> anyhow::Result<String> {
    let diff = crate::core::planning_sync::validated_git_command(repository)?
        .args(["diff", "--no-ext-diff", "--no-textconv", base, head])
        .output()?;
    anyhow::ensure!(
        diff.status.success(),
        "git could not inspect PR merge changes"
    );
    let mut command = crate::core::planning_sync::validated_git_command(repository)?;
    command.args(["patch-id", "--stable"]);
    use std::io::Write as _;
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("git patch-id input unavailable"))?
        .write_all(&diff.stdout)?;
    let output = child.wait_with_output()?;
    anyhow::ensure!(
        output.status.success(),
        "git could not identify PR merge patch"
    );
    let id = String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_owned();
    anyhow::ensure!(!id.is_empty(), "PR merge patch is empty");
    Ok(id)
}

fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_ancestor(repository: &Path, ancestor: &str, descendant: &str) -> anyhow::Result<bool> {
    let output = crate::core::planning_sync::validated_git_command(repository)?
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .output()?;
    if output.status.success() {
        return Ok(true);
    }
    anyhow::ensure!(
        output.status.code() == Some(1),
        "git could not verify PR merge ancestry"
    );
    Ok(false)
}

fn git(repository: &Path, args: &[&str]) -> anyhow::Result<String> {
    let output = crate::core::planning_sync::validated_git_command(repository)?
        .args(args)
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "git {} could not verify PR merge evidence",
        args[0]
    );
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

#[cfg(test)]
#[path = "merge_verification/tests.rs"]
mod tests;
