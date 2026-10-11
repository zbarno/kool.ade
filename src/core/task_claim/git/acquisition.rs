use super::super::{ClaimError, ClaimHistoryEntry, ClaimLease, ClaimRecord, reference};
use super::{inspect, new_record, now, push_remote, run, validate_clock, validate_remote};
use sha2::Digest;

pub fn acquire(
    repo: &std::path::Path,
    project_uid: uuid::Uuid,
    task_uid: &str,
    code_repository_id: &str,
    base_commit: &str,
) -> Result<Option<ClaimLease>, ClaimError> {
    let Some(remote) = push_remote(repo)? else {
        return Ok(None);
    };
    let reference = reference(project_uid, task_uid)?;
    validate_remote(repo, &remote)?;
    let reachable = run(repo, &["ls-remote", "--refs", &remote, "HEAD"], None)?;
    if !reachable.status.success() {
        return Err(ClaimError::RemoteUnavailable(
            String::from_utf8_lossy(&reachable.stderr).trim().to_owned(),
        ));
    }
    let record = new_record(project_uid, task_uid, code_repository_id, base_commit)?;
    let object = create_object(repo, &record)?;
    let refspec = format!("{object}:{reference}");
    let lease = format!("--force-with-lease={reference}:");
    validate_remote(repo, &remote)?;
    let pushed = run(
        repo,
        &[
            "push",
            "--no-verify",
            "--porcelain",
            &lease,
            &remote,
            &refspec,
        ],
        None,
    )?;
    if pushed.status.success() {
        return Ok(Some(lease_for(repo, remote, reference, object, &record)));
    }
    match inspect(repo, &remote, &reference) {
        Ok(Some((existing, _))) => Err(ClaimError::AlreadyClaimed {
            stale: existing.appears_stale(now()?),
            record: Box::new(existing),
        }),
        Ok(None) => Err(ClaimError::CoordinationRejected(
            String::from_utf8_lossy(&pushed.stderr).trim().to_owned(),
        )),
        Err(ClaimError::RemoteUnavailable(detail)) => Err(ClaimError::RemoteUnavailable(format!(
            "push failed and the current planning claim could not be verified: {detail}"
        ))),
        Err(error) => Err(ClaimError::CoordinationRejected(format!(
            "push failed and the current planning claim could not be verified: {error}"
        ))),
    }
}

pub fn workspace_claim_exists(
    repo: &std::path::Path,
    project_uid: uuid::Uuid,
    task_uid: &str,
) -> Result<bool, ClaimError> {
    let Some(remote) = push_remote(repo)? else {
        return Ok(false);
    };
    let reference = reference(project_uid, task_uid)?;
    let Some((record, _)) = inspect(repo, &remote, &reference)? else {
        return Ok(false);
    };
    validate_claim_key(&record, project_uid, task_uid)?;
    Ok(true)
}

pub fn take_over_stale(
    repo: &std::path::Path,
    project_uid: uuid::Uuid,
    task_uid: &str,
    code_repository_id: &str,
    expected_session: &str,
    base_commit: &str,
) -> Result<ClaimLease, ClaimError> {
    take_over_stale_inner(
        repo,
        project_uid,
        task_uid,
        code_repository_id,
        expected_session,
        base_commit,
        || {},
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub fn take_over_stale_after_prepare(
    repo: &std::path::Path,
    project_uid: uuid::Uuid,
    task_uid: &str,
    code_repository_id: &str,
    expected_session: &str,
    base_commit: &str,
    before_recheck: impl FnOnce(),
) -> Result<ClaimLease, ClaimError> {
    take_over_stale_inner(
        repo,
        project_uid,
        task_uid,
        code_repository_id,
        expected_session,
        base_commit,
        before_recheck,
    )
}

#[allow(clippy::too_many_arguments)]
fn take_over_stale_inner(
    repo: &std::path::Path,
    project_uid: uuid::Uuid,
    task_uid: &str,
    code_repository_id: &str,
    expected_session: &str,
    base_commit: &str,
    before_recheck: impl FnOnce(),
) -> Result<ClaimLease, ClaimError> {
    let reference = reference(project_uid, task_uid)?;
    let remote = push_remote(repo)?.ok_or_else(|| {
        ClaimError::RemoteUnavailable("Planning repository has no origin push remote".into())
    })?;
    let Some((record, old_object)) = inspect(repo, &remote, &reference)? else {
        return Err(ClaimError::RemoteUnavailable(
            "The planning claim no longer exists; retry normal acquisition".into(),
        ));
    };
    validate_claim_key(&record, project_uid, task_uid)?;
    let timestamp = now()?;
    validate_clock(&record, timestamp)?;
    if record.session_id != expected_session || !record.appears_stale(timestamp) {
        return Err(ClaimError::AlreadyClaimed {
            stale: record.appears_stale(timestamp),
            record: Box::new(record),
        });
    }
    let mut replacement = new_record(project_uid, task_uid, code_repository_id, base_commit)?;
    replacement.takeover_history = record.takeover_history.clone();
    replacement.takeover_history.push(ClaimHistoryEntry {
        owner: record.owner,
        session_id: record.session_id,
        code_repository_id: record.code_repository_id,
        base_commit: record.base_commit,
        claimed_at: record.issued_at_utc,
        replaced_at: timestamp,
    });
    let object = create_object(repo, &replacement)?;

    // Re-read the expected ref and expiry immediately before the CAS. Another
    // process may have renewed this claim while the replacement was prepared.
    before_recheck();
    let Some((current, current_object)) = inspect(repo, &remote, &reference)? else {
        return Err(ClaimError::RemoteUnavailable(
            "The planning claim disappeared during stale takeover".into(),
        ));
    };
    let takeover_time = now()?;
    validate_claim_key(&current, project_uid, task_uid)?;
    validate_clock(&current, takeover_time)?;
    if current_object != old_object || current.session_id != expected_session {
        return Err(ClaimError::AlreadyClaimed {
            stale: current.appears_stale(takeover_time),
            record: Box::new(current),
        });
    }
    if !current.appears_stale(takeover_time) {
        return Err(ClaimError::AlreadyClaimed {
            stale: false,
            record: Box::new(current),
        });
    }
    let lease = format!("--force-with-lease={reference}:{old_object}");
    let refspec = format!("{object}:{reference}");
    validate_remote(repo, &remote)?;
    let pushed = run(
        repo,
        &[
            "push",
            "--no-verify",
            "--porcelain",
            &lease,
            &remote,
            &refspec,
        ],
        None,
    )?;
    if pushed.status.success() {
        return Ok(lease_for(repo, remote, reference, object, &replacement));
    }
    if let Some((record, _)) = inspect(repo, &remote, &reference)? {
        return Err(ClaimError::AlreadyClaimed {
            stale: record.appears_stale(now()?),
            record: Box::new(record),
        });
    }
    Err(ClaimError::CoordinationRejected(
        String::from_utf8_lossy(&pushed.stderr).trim().to_owned(),
    ))
}

fn lease_for(
    repo: &std::path::Path,
    remote: String,
    reference: String,
    object: String,
    record: &ClaimRecord,
) -> ClaimLease {
    ClaimLease {
        repo: repo.to_owned(),
        remote,
        reference,
        object,
        project_uid: record.project_uid.clone(),
        task_uid: record.task_uid.clone(),
        code_repository_id: record.code_repository_id.clone(),
    }
}

fn validate_claim_key(
    record: &ClaimRecord,
    project_uid: uuid::Uuid,
    task_uid: &str,
) -> Result<(), ClaimError> {
    if record.project_uid != project_uid.hyphenated().to_string() || record.task_uid != task_uid {
        return Err(ClaimError::CoordinationRejected(
            "Planning claim identity does not match this project task".into(),
        ));
    }
    Ok(())
}

pub fn create_object(repo: &std::path::Path, record: &ClaimRecord) -> Result<String, ClaimError> {
    let message = serde_json::to_vec(record)
        .map_err(|error| ClaimError::RemoteUnavailable(error.to_string()))?;
    let tree = super::git(repo, &["mktree"], Some(b""))?;
    super::git(
        repo,
        &[
            "-c",
            "user.name=Kool.ad/e",
            "-c",
            "user.email=koolade-claim@noreply.invalid",
            "commit-tree",
            &tree,
        ],
        Some(&message),
    )
}

pub fn legacy_project_uid(repo: &std::path::Path) -> Result<Option<uuid::Uuid>, ClaimError> {
    let Some(remote) = push_remote(repo)? else {
        return Ok(None);
    };
    let normalized = normalize_remote_identity(&remote);
    let digest = sha2::Sha256::digest(normalized.as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(Some(uuid::Uuid::from_bytes(bytes)))
}

fn normalize_remote_identity(remote: &str) -> String {
    let parsed = if let Some((user_host, path)) = remote.split_once(':')
        && !remote.contains("://")
        && !remote.starts_with('/')
    {
        url::Url::parse(&format!("ssh://{user_host}/{path}"))
    } else {
        url::Url::parse(remote)
    };
    let Ok(mut parsed) = parsed else {
        return remote.trim().to_owned();
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let path = parsed.path().trim_end_matches(".git").trim_end_matches('/');
    let port = parsed
        .port()
        .map_or_else(String::new, |port| format!(":{port}"));
    format!("{host}{port}{path}").to_ascii_lowercase()
}
