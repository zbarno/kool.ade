use super::super::{ClaimError, ClaimRecord};
use super::{inspect, now, push_remote, run, validate_clock, validate_remote};
use std::path::PathBuf;

#[derive(Debug)]
pub(in crate::core::task_claim) struct LegacyClaim {
    pub(in crate::core::task_claim) source_repo: PathBuf,
    pub(in crate::core::task_claim) record: ClaimRecord,
    pub(in crate::core::task_claim) remote: String,
    pub(in crate::core::task_claim) reference: String,
    pub(in crate::core::task_claim) object: String,
}

/// Read an existing task-only claim for cutover compatibility. New claims are
/// always created on the project-scoped planning reference.
pub(in crate::core::task_claim) fn read_legacy_claim(
    repo: &std::path::Path,
    task_uid: &str,
) -> Result<Option<LegacyClaim>, ClaimError> {
    let reference = super::super::legacy_reference(task_uid)?;
    let Some(remote) = push_remote(repo)? else {
        return Ok(None);
    };
    let Some((record, object)) = inspect(repo, &remote, &reference)? else {
        return Ok(None);
    };
    if record.task_uid != task_uid {
        return Err(ClaimError::CoordinationRejected(
            "Prior task claim metadata does not match its Git reference".into(),
        ));
    }
    Ok(Some(LegacyClaim {
        source_repo: repo.to_owned(),
        record,
        remote,
        reference,
        object,
    }))
}

/// Remove a prior-schema claim only after an explicit stale takeover. The
/// expected object and expiry are rechecked before the remote CAS delete.
pub(in crate::core::task_claim) fn remove_expired_legacy_claim(
    repo: &std::path::Path,
    claim: &LegacyClaim,
    expected_session: &str,
) -> Result<(), ClaimError> {
    let Some((record, object)) = inspect(repo, &claim.remote, &claim.reference)? else {
        return Ok(());
    };
    let timestamp = now()?;
    validate_clock(&record, timestamp)?;
    if object != claim.object || record.session_id != expected_session {
        return Err(ClaimError::AlreadyClaimed {
            stale: record.appears_stale(timestamp),
            record: Box::new(record),
        });
    }
    if !record.appears_stale(timestamp) {
        return Err(ClaimError::AlreadyClaimed {
            stale: false,
            record: Box::new(record),
        });
    }
    let lease = format!("--force-with-lease={}:{object}", claim.reference);
    validate_remote(repo, &claim.remote)?;
    let deleted = run(
        repo,
        &[
            "push",
            "--porcelain",
            &lease,
            &claim.remote,
            &format!(":{}", claim.reference),
        ],
        None,
    )?;
    if deleted.status.success() {
        return Ok(());
    }
    if let Some((record, _)) = inspect(repo, &claim.remote, &claim.reference)? {
        return Err(ClaimError::AlreadyClaimed {
            stale: record.appears_stale(now()?),
            record: Box::new(record),
        });
    }
    Err(ClaimError::CoordinationRejected(
        String::from_utf8_lossy(&deleted.stderr).trim().to_owned(),
    ))
}
