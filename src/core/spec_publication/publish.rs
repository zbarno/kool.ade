use super::git::*;
use super::github::*;
use super::validation::*;
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::Path;

/// Publish a previously approved document set through an isolated branch and PR.
/// A fresh consent token and immutable published-spec pin are required on every call.
pub(crate) fn publish_approved(
    repository_root: &Path,
    repository_id: &str,
    pin: &PublishedSpecPin,
    changes: &[DocumentationChange],
    consent: &PublicationConsent,
) -> anyhow::Result<PullRequest> {
    publish_inner(
        repository_root,
        repository_id,
        pin,
        changes,
        consent,
        std::ffi::OsStr::new("gh"),
        false,
    )
}

#[cfg(test)]
pub(super) fn publish_with_gh(
    repository_root: &Path,
    repository_id: &str,
    pin: &PublishedSpecPin,
    changes: &[DocumentationChange],
    consent: &PublicationConsent,
    gh: &std::ffi::OsStr,
) -> anyhow::Result<PullRequest> {
    publish_inner(
        repository_root,
        repository_id,
        pin,
        changes,
        consent,
        gh,
        true,
    )
}

fn publish_inner(
    repository_root: &Path,
    repository_id: &str,
    pin: &PublishedSpecPin,
    changes: &[DocumentationChange],
    consent: &PublicationConsent,
    gh: &std::ffi::OsStr,
    allow_test_remote: bool,
) -> anyhow::Result<PullRequest> {
    validate_changes(changes)?;
    anyhow::ensure!(
        consent.approved_document_digest == digest_changes(changes),
        "publication confirmation does not match the approved documentation"
    );

    let reader = PublishedSpecReader::new(
        repository_root.to_path_buf(),
        repository_id.to_owned(),
        pin.reference.clone(),
    )?;
    reader.ensure_local_ref_unchanged(pin)?;
    let base_branch = branch_from_reference(&pin.reference)?;
    let expected_remote_ref = format!("refs/heads/{base_branch}");
    let remote_url = trusted_origin_url(repository_root)?;
    let remote_repository = github_repository_from_remote(&remote_url)?;
    anyhow::ensure!(
        remote_repository.is_some() || allow_test_remote,
        "origin must identify the GitHub repository used for publication"
    );
    let remote_sha = remote_branch_sha(repository_root, &expected_remote_ref)?;
    anyhow::ensure!(
        remote_sha == pin.commit_sha,
        "published documentation changed on the remote; review the new baseline before publishing"
    );
    let repository = github_repository(repository_root, gh)?;
    if let Some(remote_repository) = remote_repository {
        anyhow::ensure!(
            remote_repository == repository,
            "GitHub CLI repository does not match the origin publication remote"
        );
    }
    let identity = public_commit_identity(gh, repository_root)?;

    let temp = crate::persistence::temporary_directory("koolade-spec-publication-")?;
    let bare = temp.path().join("publication");
    git_standalone(&["init", bare.to_str().unwrap_or_default()])?;
    git_at(&bare, &["remote", "add", "origin", &remote_url])?;
    anyhow::ensure!(
        git_at(&bare, &["remote", "get-url", "--all", "origin"])? == remote_url
            && git_at(&bare, &["remote", "get-url", "--push", "--all", "origin"])? == remote_url,
        "Git configuration would redirect publication to a different remote"
    );
    let local_base_ref = "refs/koolade/published-base";
    git_at(
        &bare,
        &[
            "fetch",
            "--no-tags",
            "origin",
            &format!("{expected_remote_ref}:{local_base_ref}"),
        ],
    )?;
    let fetched_sha = git_at(
        &bare,
        &[
            "rev-parse",
            "--verify",
            &format!("{local_base_ref}^{{commit}}"),
        ],
    )?;
    anyhow::ensure!(
        fetched_sha == pin.commit_sha,
        "published documentation moved while preparing the isolated publication"
    );
    anyhow::ensure!(
        remote_branch_sha(repository_root, &expected_remote_ref)? == pin.commit_sha,
        "published documentation moved during publication preparation"
    );

    let mut seen = BTreeSet::new();
    let index = temp.path().join("index");
    git_at_env(&bare, &["read-tree", &pin.commit_sha], Some(&index))?;
    for change in changes {
        ensure_existing_path_is_safe(&bare, &pin.commit_sha, &change.path)?;
        let blob = git_input_at(
            &bare,
            &["hash-object", "-w", "--stdin"],
            change.content.as_bytes(),
        )?;
        let entry = format!("100644 {blob} 0\t{}\n", change.path);
        git_input_at_env(
            &bare,
            &["update-index", "--add", "--index-info"],
            entry.as_bytes(),
            Some(&index),
        )?;
        seen.insert(change.path.as_str());
    }
    let tree = git_at_env(&bare, &["write-tree"], Some(&index))?;
    let mut branch_digest = Sha256::new();
    branch_digest.update(pin.commit_sha.as_bytes());
    branch_digest.update(consent.approved_document_digest.as_bytes());
    let branch_id = format!("{:x}", branch_digest.finalize());
    let branch = format!("docs/koolade-spec-{}", &branch_id[..16]);
    validate_branch(&branch)?;
    let proposed_head = git_commit_tree(&bare, &tree, &pin.commit_sha, &identity)?;
    let remote_branch_ref = format!("refs/heads/{branch}");
    let head = if let Some(existing_head) =
        remote_branch_sha_optional(repository_root, &remote_branch_ref)?
    {
        let existing_ref = "refs/koolade/existing-publication";
        git_at(
            &bare,
            &[
                "fetch",
                "--no-tags",
                "origin",
                &format!("{remote_branch_ref}:{existing_ref}"),
            ],
        )?;
        let existing_tree = git_at(
            &bare,
            &["rev-parse", "--verify", &format!("{existing_ref}^{{tree}}")],
        )?;
        let existing_parent = git_at(
            &bare,
            &["rev-parse", "--verify", &format!("{existing_ref}^")],
        )?;
        let fetched_head = git_at(
            &bare,
            &[
                "rev-parse",
                "--verify",
                &format!("{existing_ref}^{{commit}}"),
            ],
        )?;
        anyhow::ensure!(
            existing_head == fetched_head
                && existing_tree == tree
                && existing_parent == pin.commit_sha,
            "existing publication branch contains a different approved change set"
        );
        existing_head
    } else {
        git_at(
            &bare,
            &[
                "push",
                "--porcelain",
                "origin",
                &format!("{proposed_head}:{remote_branch_ref}"),
            ],
        )?;
        proposed_head
    };

    let body_path = temp.path().join("pull-request.md");
    let body = format!(
        "## Summary\n\nPublish {} approved Kool.ad/e documentation file(s).\n\nBase: `{}`\n\nThe proposal contains only product, feature, or architecture Markdown. Normal repository review and branch protections apply.",
        seen.len(),
        pin.commit_sha
    );
    std::fs::write(&body_path, body)?;
    anyhow::ensure!(
        remote_branch_sha(repository_root, &expected_remote_ref)? == pin.commit_sha,
        "published documentation advanced before pull request creation; review the new baseline before publishing"
    );
    let url = create_pull_request(
        repository_root,
        gh,
        &repository,
        PullRequestTarget {
            base: &base_branch,
            head: &branch,
            head_sha: &head,
            base_ref: &expected_remote_ref,
            base_sha: &pin.commit_sha,
        },
        &body_path,
    )?;
    Ok(PullRequest {
        url,
        branch,
        base_commit: pin.commit_sha.clone(),
        head_commit: head,
    })
}
