use super::super::{ClaimError, ClaimExecutionAttempt, ClaimExecutionAttemptStatus, ClaimRecord};
use super::{LEASE_SECONDS, inspect, now, run, validate_clock, validate_remote};

pub fn read_claim(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
    expected_object: &str,
) -> Result<ClaimRecord, ClaimError> {
    let Some((record, actual_object)) = inspect(repo, remote, reference)? else {
        return Err(ClaimError::RemoteUnavailable(
            "Claim disappeared before it could be read".into(),
        ));
    };
    if actual_object != expected_object {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim was replaced by another session".into(),
        ));
    }
    Ok(record)
}

pub fn verify(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
    expected_object: &str,
) -> Result<(), ClaimError> {
    let Some((record, actual_object)) = inspect(repo, remote, reference)? else {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim disappeared before publication".into(),
        ));
    };
    if actual_object != expected_object {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim was replaced by another session".into(),
        ));
    }
    let timestamp = now()?;
    validate_clock(&record, timestamp)?;
    if record.appears_stale(timestamp) {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim expired before publication".into(),
        ));
    }
    Ok(())
}

pub fn refresh(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
    expected_object: &str,
) -> Result<String, ClaimError> {
    let Some((mut record, actual_object)) = inspect(repo, remote, reference)? else {
        return Err(ClaimError::RemoteUnavailable(
            "The planning task claim disappeared".into(),
        ));
    };
    if actual_object != expected_object {
        return Err(ClaimError::RemoteUnavailable(
            "The planning task claim was replaced by another session".into(),
        ));
    }
    let timestamp = now()?;
    validate_clock(&record, timestamp)?;
    if record.appears_stale(timestamp) {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim expired before it could be renewed".into(),
        ));
    }
    record.expires_at_utc = timestamp.saturating_add(LEASE_SECONDS);
    let object = super::acquisition::create_object(repo, &record)?;
    let lease = format!("--force-with-lease={reference}:{expected_object}");
    let refspec = format!("{object}:{reference}");
    validate_remote(repo, remote)?;
    let pushed = run(
        repo,
        &[
            "push",
            "--no-verify",
            "--porcelain",
            &lease,
            remote,
            &refspec,
        ],
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

pub fn begin_execution_attempt(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
    expected_object: &str,
    attempt: ClaimExecutionAttempt,
) -> Result<String, ClaimError> {
    attempt.validate()?;
    let Some((mut record, actual_object)) = inspect(repo, remote, reference)? else {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim disappeared before remote execution".into(),
        ));
    };
    if actual_object != expected_object {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim was replaced before remote execution".into(),
        ));
    }
    if record
        .execution_attempt
        .as_ref()
        .is_some_and(|active| active.status == ClaimExecutionAttemptStatus::Running)
    {
        return Err(ClaimError::CoordinationRejected(
            "A remote worker attempt is still running; confirm its loss before retrying".into(),
        ));
    }
    let timestamp = now()?;
    validate_clock(&record, timestamp)?;
    if record.appears_stale(timestamp) {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim expired before remote execution".into(),
        ));
    }
    if attempt.started_at_utc > timestamp {
        return Err(ClaimError::CoordinationRejected(
            "Remote execution attempt starts in the future".into(),
        ));
    }
    record.execution_attempt = Some(attempt);
    record.expires_at_utc = timestamp.saturating_add(LEASE_SECONDS);
    push_claim_update(repo, remote, reference, expected_object, &record)
}

pub fn set_execution_attempt_status(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
    expected_object: &str,
    attempt_id: &str,
    status: ClaimExecutionAttemptStatus,
) -> Result<String, ClaimError> {
    let Some((mut record, actual_object)) = inspect(repo, remote, reference)? else {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim disappeared during remote execution".into(),
        ));
    };
    if actual_object != expected_object {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim was replaced during remote execution".into(),
        ));
    }
    let timestamp = now()?;
    validate_clock(&record, timestamp)?;
    if record.appears_stale(timestamp) {
        return Err(ClaimError::CoordinationRejected(
            "The planning task claim expired during remote execution".into(),
        ));
    }
    let attempt = record
        .execution_attempt
        .as_mut()
        .filter(|attempt| attempt.attempt_id == attempt_id)
        .ok_or_else(|| {
            ClaimError::CoordinationRejected(
                "Remote execution attempt was replaced by another attempt".into(),
            )
        })?;
    let transition_allowed = matches!(
        (attempt.status, status),
        (
            ClaimExecutionAttemptStatus::Prepared,
            ClaimExecutionAttemptStatus::Running
        ) | (
            ClaimExecutionAttemptStatus::Running,
            ClaimExecutionAttemptStatus::Finished
        ) | (
            ClaimExecutionAttemptStatus::Running,
            ClaimExecutionAttemptStatus::ConfirmedLost
        ) | (
            ClaimExecutionAttemptStatus::Finished,
            ClaimExecutionAttemptStatus::Running
        )
    ) || attempt.status == status;
    if !transition_allowed {
        return Err(ClaimError::CoordinationRejected(
            "Remote execution attempt status transition is invalid".into(),
        ));
    }
    attempt.status = status;
    record.expires_at_utc = timestamp.saturating_add(LEASE_SECONDS);
    push_claim_update(repo, remote, reference, expected_object, &record)
}

fn push_claim_update(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
    expected_object: &str,
    record: &ClaimRecord,
) -> Result<String, ClaimError> {
    let object = super::acquisition::create_object(repo, record)?;
    let lease = format!("--force-with-lease={reference}:{expected_object}");
    let refspec = format!("{object}:{reference}");
    validate_remote(repo, remote)?;
    let pushed = run(
        repo,
        &[
            "push",
            "--no-verify",
            "--porcelain",
            &lease,
            remote,
            &refspec,
        ],
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

pub fn release(
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
            "--no-verify",
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
