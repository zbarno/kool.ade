use super::{ClaimError, ClaimHistoryEntry, ClaimRecord};
use std::process::{Command, Output};

mod acquisition;
mod lease;
mod legacy;
mod publication;

#[cfg(test)]
pub(super) use acquisition::create_object;
#[cfg(test)]
pub(super) use acquisition::take_over_stale_after_prepare;
pub(super) use acquisition::{
    acquire, legacy_project_uid, take_over_stale, workspace_claim_exists,
};
pub(super) use lease::{read_claim, refresh, release, verify};
pub(super) use legacy::{LegacyClaim, read_legacy_claim, remove_expired_legacy_claim};
pub(super) use publication::fenced_push;
#[cfg(test)]
pub(super) use publication::fenced_push_after_prepare;

pub(super) const LEASE_SECONDS: i64 = 15 * 60;

pub(super) fn run(
    repo: &std::path::Path,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<Output, ClaimError> {
    let mut command = Command::new("git");
    command.args(args).current_dir(repo);
    if input.is_some() {
        command.stdin(std::process::Stdio::piped());
    }
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| ClaimError::RemoteUnavailable(error.to_string()))?;
    if let Some(bytes) = input {
        use std::io::Write;
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(bytes)
            .map_err(|error| ClaimError::RemoteUnavailable(error.to_string()))?;
    }
    child
        .wait_with_output()
        .map_err(|error| ClaimError::RemoteUnavailable(error.to_string()))
}

pub(super) fn git(
    repo: &std::path::Path,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<String, ClaimError> {
    let output = run(repo, args, input)?;
    if !output.status.success() {
        return Err(ClaimError::RemoteUnavailable(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    String::from_utf8(output.stdout)
        .map(|value| value.trim().to_owned())
        .map_err(|error| ClaimError::RemoteUnavailable(error.to_string()))
}

pub(super) fn push_remote(repo: &std::path::Path) -> Result<Option<String>, ClaimError> {
    let output = run(
        repo,
        &["remote", "get-url", "--push", "--all", "origin"],
        None,
    )?;
    if !output.status.success() {
        return Ok(None);
    }
    let output = String::from_utf8(output.stdout)
        .map_err(|error| ClaimError::RemoteUnavailable(error.to_string()))?;
    let urls = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    match urls.as_slice() {
        [] => Ok(None),
        [url] => Ok(Some((*url).to_owned())),
        _ => Err(ClaimError::CoordinationRejected(
            "Repositories with multiple origin push URLs cannot hold a single task claim".into(),
        )),
    }
}

pub(super) fn validate_remote(repo: &std::path::Path, expected: &str) -> Result<(), ClaimError> {
    if push_remote(repo)?.as_deref() != Some(expected) {
        return Err(ClaimError::CoordinationRejected(
            "The repository push destination changed during task coordination".into(),
        ));
    }
    Ok(())
}

pub(super) fn inspect(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
) -> Result<Option<(ClaimRecord, String)>, ClaimError> {
    validate_remote(repo, remote)?;
    let listing = run(repo, &["ls-remote", "--refs", remote, reference], None)?;
    if !listing.status.success() {
        return Err(ClaimError::RemoteUnavailable(
            String::from_utf8_lossy(&listing.stderr).trim().to_owned(),
        ));
    }
    let Some(object) = String::from_utf8_lossy(&listing.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
    else {
        return Ok(None);
    };
    let fetched = run(
        repo,
        &[
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            remote,
            reference,
        ],
        None,
    )?;
    if !fetched.status.success() {
        return Err(ClaimError::RemoteUnavailable(
            String::from_utf8_lossy(&fetched.stderr).trim().to_owned(),
        ));
    }
    let body = git(repo, &["show", "-s", "--format=%B", &object], None)?;
    serde_json::from_str(&body)
        .map(|record| Some((record, object)))
        .map_err(|error| {
            ClaimError::CoordinationRejected(format!(
                "Planning remote claim metadata is invalid: {error}"
            ))
        })
}

pub(super) fn now() -> Result<i64, ClaimError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(|_| {
            ClaimError::CoordinationRejected(
                "System clock is before the UTC Unix epoch; task coordination is unsafe".into(),
            )
        })
}

pub(super) fn validate_clock(record: &ClaimRecord, timestamp: i64) -> Result<(), ClaimError> {
    if record.issued_at_utc > timestamp {
        return Err(ClaimError::CoordinationRejected(
            "System clock is earlier than the task claim issue time; refusing to coordinate".into(),
        ));
    }
    if record.expires_at() < record.issued_at_utc {
        return Err(ClaimError::CoordinationRejected(
            "Task claim has an invalid UTC lease interval".into(),
        ));
    }
    Ok(())
}

pub(super) fn new_record(
    project_uid: uuid::Uuid,
    task_uid: &str,
    code_repository_id: &str,
    base_commit: &str,
) -> Result<ClaimRecord, ClaimError> {
    if !matches!(base_commit.len(), 40 | 64)
        || !base_commit.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ClaimError::CoordinationRejected(
            "Base commit identity must be a Git object SHA".into(),
        ));
    }
    let issued_at_utc = now()?;
    Ok(ClaimRecord {
        schema_version: 3,
        project_uid: project_uid.hyphenated().to_string(),
        task_uid: task_uid.to_owned(),
        owner: "Kool.ad/e session".to_owned(),
        operator_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        session_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        code_repository_id: code_repository_id.to_owned(),
        base_commit: base_commit.to_owned(),
        issued_at_utc,
        expires_at_utc: issued_at_utc.saturating_add(LEASE_SECONDS),
        takeover_history: Box::<Vec<ClaimHistoryEntry>>::default(),
    })
}
