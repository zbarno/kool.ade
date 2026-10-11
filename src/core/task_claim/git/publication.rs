use super::super::ClaimError;
use super::{LEASE_SECONDS, git, now, push_remote, run, validate_clock, validate_remote};

struct FencedPush<'a> {
    planning_repo: &'a std::path::Path,
    planning_remote: &'a str,
    reference: &'a str,
    expected_object: &'a str,
    code_repo: &'a std::path::Path,
    commit: &'a str,
    destination_ref: &'a str,
}

pub fn fenced_push(
    planning_repo: &std::path::Path,
    planning_remote: &str,
    reference: &str,
    expected_object: &str,
    code_repo: &std::path::Path,
    commit: &str,
    destination_ref: &str,
) -> Result<String, ClaimError> {
    fenced_push_inner(
        FencedPush {
            planning_repo,
            planning_remote,
            reference,
            expected_object,
            code_repo,
            commit,
            destination_ref,
        },
        || {},
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn fenced_push_after_prepare(
    planning_repo: &std::path::Path,
    planning_remote: &str,
    reference: &str,
    expected_object: &str,
    code_repo: &std::path::Path,
    commit: &str,
    destination_ref: &str,
    before_push: impl FnOnce(),
) -> Result<String, ClaimError> {
    fenced_push_inner(
        FencedPush {
            planning_repo,
            planning_remote,
            reference,
            expected_object,
            code_repo,
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
        planning_repo,
        planning_remote,
        reference,
        expected_object,
        code_repo,
        commit,
        destination_ref,
    } = push;
    if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ClaimError::CoordinationRejected(
            "Candidate commit identity is invalid".into(),
        ));
    }
    validate_remote(planning_repo, planning_remote)?;
    let _ = git(code_repo, &["check-ref-format", destination_ref], None)?;
    let code_remote = push_remote(code_repo)?.ok_or_else(|| {
        ClaimError::RemoteUnavailable("Task code repository has no origin push remote".into())
    })?;
    validate_remote(code_repo, &code_remote)?;
    let _ = git(
        code_repo,
        &["cat-file", "-e", &format!("{commit}^{{commit}}")],
        None,
    )?;

    if same_remote_target(planning_remote, &code_remote) {
        return fenced_push_same_remote(
            planning_repo,
            planning_remote,
            reference,
            expected_object,
            code_repo,
            &code_remote,
            commit,
            destination_ref,
            before_push,
        );
    }

    // The claim and code repository may use different Git providers. They
    // cannot participate in one atomic push, so verify immediately before the
    // code push and again after any potentially slow preparation.
    super::lease::verify(planning_repo, planning_remote, reference, expected_object)?;
    before_push();
    super::lease::verify(planning_repo, planning_remote, reference, expected_object)?;
    validate_remote(code_repo, &code_remote)?;
    let candidate = format!("{commit}:{destination_ref}");
    let pushed = run(
        code_repo,
        &[
            "push",
            "--no-verify",
            "--porcelain",
            &code_remote,
            &candidate,
        ],
        None,
    )?;
    if pushed.status.success() {
        Ok(expected_object.to_owned())
    } else {
        Err(ClaimError::CoordinationRejected(
            String::from_utf8_lossy(&pushed.stderr).trim().to_owned(),
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn fenced_push_same_remote(
    planning_repo: &std::path::Path,
    planning_remote: &str,
    reference: &str,
    expected_object: &str,
    code_repo: &std::path::Path,
    code_remote: &str,
    commit: &str,
    destination_ref: &str,
    before_push: impl FnOnce(),
) -> Result<String, ClaimError> {
    let Some((mut record, object)) = super::inspect(code_repo, code_remote, reference)? else {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim disappeared before atomic publication".into(),
        ));
    };
    let timestamp = now()?;
    validate_clock(&record, timestamp)?;
    if object != expected_object || record.appears_stale(timestamp) {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim changed or expired before atomic publication".into(),
        ));
    }
    record.expires_at_utc = timestamp.saturating_add(LEASE_SECONDS);
    let renewed_object = super::acquisition::create_object(code_repo, &record)?;
    before_push();

    // For a shared push destination, update the claim and code branch in one
    // remote transaction. The expected claim SHA fences a takeover that races
    // after the last read above.
    validate_remote(planning_repo, planning_remote)?;
    validate_remote(code_repo, code_remote)?;
    let lease = format!("--force-with-lease={reference}:{expected_object}");
    let candidate = format!("{commit}:{destination_ref}");
    let claim = format!("{renewed_object}:{reference}");
    let pushed = run(
        code_repo,
        &[
            "push",
            "--no-verify",
            "--atomic",
            "--porcelain",
            &lease,
            code_remote,
            &candidate,
            &claim,
        ],
        None,
    )?;
    if pushed.status.success() {
        Ok(renewed_object)
    } else {
        Err(ClaimError::CoordinationRejected(
            String::from_utf8_lossy(&pushed.stderr).trim().to_owned(),
        ))
    }
}

fn same_remote_target(left: &str, right: &str) -> bool {
    if left == right {
        return true;
    }
    match (push_target_identity(left), push_target_identity(right)) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}

fn push_target_identity(remote: &str) -> Option<String> {
    let address = if let Some((user_host, path)) = remote.split_once(':')
        && !remote.contains("://")
        && !remote.starts_with('/')
    {
        format!("ssh://{user_host}/{path}")
    } else {
        remote.to_owned()
    };
    let parsed = url::Url::parse(&address).ok()?;
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let port = parsed
        .port()
        .map_or_else(String::new, |port| format!(":{port}"));
    let path = parsed.path().trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    Some(format!("{host}{port}{path}"))
}
