use super::super::{ClaimError, ClaimRecord};
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
