use super::super::{ClaimError, git, run, validate_remote};

#[allow(clippy::too_many_arguments)]
pub(super) fn push(
    planning_repo: &std::path::Path,
    planning_remote: &str,
    reference: &str,
    expected_object: &str,
    code_repo: &std::path::Path,
    code_remote: &str,
    commit: &str,
    destination_ref: &str,
    before_push: impl FnOnce(),
    after_push: impl FnOnce(),
    simulate_lost_ack: bool,
) -> Result<String, ClaimError> {
    super::super::lease::verify(planning_repo, planning_remote, reference, expected_object)?;
    before_push();
    super::super::lease::verify(planning_repo, planning_remote, reference, expected_object)?;
    validate_remote(code_repo, code_remote)?;
    let record = super::super::lease::read_claim(
        planning_repo,
        planning_remote,
        reference,
        expected_object,
    )?;
    let fence_ref = fence_ref(reference)?;
    let empty_tree = git(code_repo, &["mktree"], Some(b""))?;
    let fence_object = git(
        code_repo,
        &[
            "-c",
            "user.name=Kool.ad/e Claim Fence",
            "-c",
            "user.email=5166344+zbarno@users.noreply.github.com",
            "commit-tree",
            &empty_tree,
            "-m",
            &record.session_id,
        ],
        None,
    )?;
    let prior_branch = remote_ref_object(code_repo, code_remote, destination_ref)?;
    let prior_fence = remote_ref_object(code_repo, code_remote, &fence_ref)?;
    if let Some(existing) = prior_branch.as_deref()
        && existing != commit
    {
        let fetched = run(
            code_repo,
            &[
                "fetch",
                "--no-tags",
                "--no-write-fetch-head",
                code_remote,
                destination_ref,
            ],
            None,
        )?;
        if !fetched.status.success() {
            return Err(ClaimError::RemoteUnavailable(
                String::from_utf8_lossy(&fetched.stderr).trim().to_owned(),
            ));
        }
        let ancestor = run(
            code_repo,
            &["merge-base", "--is-ancestor", existing, commit],
            None,
        )?;
        if !ancestor.status.success() {
            return Err(ClaimError::CoordinationRejected(
                "The task candidate does not extend the existing code branch".into(),
            ));
        }
    }

    // The session marker gives cleanup an ownership token independent of the
    // candidate commit, which can be reused by a later task owner.
    super::super::lease::verify(planning_repo, planning_remote, reference, expected_object)?;
    let branch_lease = format!(
        "--force-with-lease={destination_ref}:{}",
        prior_branch.as_deref().unwrap_or_default()
    );
    let fence_lease = format!(
        "--force-with-lease={fence_ref}:{}",
        prior_fence.as_deref().unwrap_or_default()
    );
    let candidate = format!("{commit}:{destination_ref}");
    let fence = format!("{fence_object}:{fence_ref}");
    let pushed = run(
        code_repo,
        &[
            "push",
            "--atomic",
            "--no-verify",
            "--porcelain",
            &branch_lease,
            &fence_lease,
            code_remote,
            &candidate,
            &fence,
        ],
        None,
    )?;
    let applied = (pushed.status.success() && !simulate_lost_ack)
        || (remote_ref_object(code_repo, code_remote, destination_ref)?.as_deref() == Some(commit)
            && remote_ref_object(code_repo, code_remote, &fence_ref)?.as_deref()
                == Some(fence_object.as_str()));
    if !applied {
        return Err(ClaimError::CoordinationRejected(format!(
            "Code branch and ownership fence were not confirmed at the code remote: {}",
            String::from_utf8_lossy(&pushed.stderr).trim()
        )));
    }

    after_push();
    if let Err(lease_error) =
        super::super::lease::verify(planning_repo, planning_remote, reference, expected_object)
    {
        let cleanup = retract_candidate(
            code_repo,
            code_remote,
            commit,
            destination_ref,
            prior_branch.as_deref(),
            &fence_object,
            &fence_ref,
        );
        return match cleanup {
            Ok(()) => Err(lease_error),
            Err(cleanup_error) => Err(ClaimError::CoordinationRejected(format!(
                "The planning task claim was lost after the code branch push ({lease_error}); the branch could not be safely retracted ({cleanup_error})"
            ))),
        };
    }
    Ok(expected_object.to_owned())
}

fn fence_ref(claim_ref: &str) -> Result<String, ClaimError> {
    let key = claim_ref
        .strip_prefix("refs/heads/koolade/claims/")
        .ok_or_else(|| {
            ClaimError::CoordinationRejected(
                "Planning task claim ref cannot be mapped to a code ownership fence".into(),
            )
        })?;
    Ok(format!("refs/heads/koolade/claim-fences/{key}"))
}

fn remote_ref_object(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
) -> Result<Option<String>, ClaimError> {
    validate_remote(repo, remote)?;
    let output = run(repo, &["ls-remote", "--refs", remote, reference], None)?;
    if !output.status.success() {
        return Err(ClaimError::RemoteUnavailable(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned))
}

fn retract_candidate(
    repo: &std::path::Path,
    remote: &str,
    commit: &str,
    reference: &str,
    previous: Option<&str>,
    fence_object: &str,
    fence_ref: &str,
) -> Result<(), ClaimError> {
    validate_remote(repo, remote)?;
    let lease = format!("--force-with-lease={reference}:{commit}");
    let fence_lease = format!("--force-with-lease={fence_ref}:{fence_object}");
    let branch_refspec = previous.map_or_else(
        || format!(":{reference}"),
        |old| format!("{old}:{reference}"),
    );
    let fence_refspec = format!(":{fence_ref}");
    let removed = run(
        repo,
        &[
            "push",
            "--atomic",
            "--no-verify",
            "--porcelain",
            &lease,
            &fence_lease,
            remote,
            &branch_refspec,
            &fence_refspec,
        ],
        None,
    )?;
    if removed.status.success() {
        Ok(())
    } else {
        Err(ClaimError::CoordinationRejected(
            String::from_utf8_lossy(&removed.stderr).trim().to_owned(),
        ))
    }
}
