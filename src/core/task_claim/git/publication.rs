use super::*;

struct FencedPush<'a> {
    repo: &'a std::path::Path,
    remote: &'a str,
    reference: &'a str,
    expected_object: &'a str,
    source: &'a std::path::Path,
    commit: &'a str,
    destination_ref: &'a str,
}

pub(crate) fn fenced_push(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
    expected_object: &str,
    source: &std::path::Path,
    commit: &str,
    destination_ref: &str,
) -> Result<String, ClaimError> {
    fenced_push_inner(
        FencedPush {
            repo,
            remote,
            reference,
            expected_object,
            source,
            commit,
            destination_ref,
        },
        || {},
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn fenced_push_after_prepare(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
    expected_object: &str,
    source: &std::path::Path,
    commit: &str,
    destination_ref: &str,
    before_push: impl FnOnce(),
) -> Result<String, ClaimError> {
    fenced_push_inner(
        FencedPush {
            repo,
            remote,
            reference,
            expected_object,
            source,
            commit,
            destination_ref,
        },
        before_push,
    )
}

fn fenced_push_inner(
    push: FencedPush<'_>,
    before_push: impl FnOnce(),
) -> Result<String, ClaimError> {
    let FencedPush {
        repo,
        remote,
        reference,
        expected_object,
        source,
        commit,
        destination_ref,
    } = push;
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ClaimError::CoordinationRejected(
            "Candidate commit identity is invalid".into(),
        ));
    }
    validate_remote(repo, remote)?;
    let _ = git(repo, &["check-ref-format", destination_ref], None)?;
    let record = read_claim(repo, remote, reference, expected_object)?;
    let source = source
        .to_str()
        .ok_or_else(|| ClaimError::CoordinationRejected("Non-UTF8 task repository path".into()))?;
    let fetched = run(
        repo,
        &[
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            source,
            commit,
        ],
        None,
    )?;
    if !fetched.status.success() {
        return Err(ClaimError::CoordinationRejected(
            String::from_utf8_lossy(&fetched.stderr).trim().to_owned(),
        ));
    }
    let _ = git(
        repo,
        &["cat-file", "-e", &format!("{commit}^{{commit}}")],
        None,
    )?;
    let mut renewed = record;
    renewed.claimed_at = now();
    let next_object = create_object(repo, &renewed)?;
    let lease = format!("--force-with-lease={reference}:{expected_object}");
    let candidate = format!("{commit}:{destination_ref}");
    let claim = format!("{next_object}:{reference}");
    before_push();
    validate_remote(repo, remote)?;
    let pushed = run(
        repo,
        &[
            "push",
            "--atomic",
            "--porcelain",
            &lease,
            remote,
            &candidate,
            &claim,
        ],
        None,
    )?;
    if pushed.status.success() {
        Ok(next_object)
    } else {
        Err(ClaimError::CoordinationRejected(
            String::from_utf8_lossy(&pushed.stderr).trim().to_owned(),
        ))
    }
}

pub(crate) fn refresh(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
    expected_object: &str,
) -> Result<String, ClaimError> {
    let Some((mut record, actual_object)) = inspect(repo, remote, reference)? else {
        return Err(ClaimError::RemoteUnavailable(
            "The task claim disappeared".into(),
        ));
    };
    if actual_object != expected_object {
        return Err(ClaimError::RemoteUnavailable(
            "The task claim was replaced by another session".into(),
        ));
    }
    record.claimed_at = now();
    let object = create_object(repo, &record)?;
    let lease = format!("--force-with-lease={reference}:{expected_object}");
    let refspec = format!("{object}:{reference}");
    validate_remote(repo, remote)?;
    let pushed = run(
        repo,
        &["push", "--porcelain", &lease, remote, &refspec],
        None,
    )?;
    if pushed.status.success() {
        Ok(object)
    } else {
        Err(ClaimError::CoordinationRejected(
            String::from_utf8_lossy(&pushed.stderr).trim().to_owned(),
        ))
    }
}

pub(crate) fn release(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
    object: &str,
) -> Result<(), ClaimError> {
    validate_remote(repo, remote)?;
    let listing = run(repo, &["ls-remote", "--refs", remote, reference], None)?;
    if !listing.status.success() {
        return Err(ClaimError::RemoteUnavailable(
            String::from_utf8_lossy(&listing.stderr).trim().to_owned(),
        ));
    }
    if !String::from_utf8_lossy(&listing.stdout).starts_with(object) {
        return Ok(());
    }
    let lease = format!("--force-with-lease={reference}:{object}");
    validate_remote(repo, remote)?;
    let result = run(
        repo,
        &[
            "push",
            "--porcelain",
            &lease,
            remote,
            &format!(":{reference}"),
        ],
        None,
    )?;
    if result.status.success() {
        Ok(())
    } else {
        Err(ClaimError::RemoteUnavailable(
            String::from_utf8_lossy(&result.stderr).trim().to_owned(),
        ))
    }
}
