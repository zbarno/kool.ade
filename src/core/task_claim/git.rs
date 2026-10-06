use super::*;
use std::{
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn run(repo: &std::path::Path, args: &[&str], input: Option<&[u8]>) -> Result<Output, ClaimError> {
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

fn git(repo: &std::path::Path, args: &[&str], input: Option<&[u8]>) -> Result<String, ClaimError> {
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

pub(super) fn acquire(
    repo: &std::path::Path,
    task_uid: &str,
    base_commit: &str,
) -> Result<Option<ClaimLease>, ClaimError> {
    let reference = super::reference(task_uid)
        .map_err(|error| ClaimError::RemoteUnavailable(error.to_string()))?;
    let remote = run(repo, &["remote", "get-url", "origin"], None)?;
    if !remote.status.success() {
        return Ok(None);
    }
    let reachable = run(repo, &["ls-remote", "--refs", "origin", "HEAD"], None)?;
    if !reachable.status.success() {
        return Err(ClaimError::RemoteUnavailable(
            String::from_utf8_lossy(&reachable.stderr).trim().to_owned(),
        ));
    }
    let record = new_record(task_uid, base_commit);
    let object = create_object(repo, &record)?;
    let refspec = format!("{object}:{reference}");
    let lease = format!("--force-with-lease={reference}:");
    let pushed = run(
        repo,
        &["push", "--porcelain", &lease, "origin", &refspec],
        None,
    )?;
    if pushed.status.success() {
        return Ok(Some(ClaimLease {
            repo: repo.to_owned(),
            reference,
            object,
        }));
    }
    match inspect(repo, &reference) {
        Ok(Some(existing)) => Err(ClaimError::AlreadyClaimed {
            stale: existing.0.appears_stale(now()),
            record: existing.0,
        }),
        Ok(None) => Err(ClaimError::CoordinationRejected(
            String::from_utf8_lossy(&pushed.stderr).trim().to_owned(),
        )),
        Err(error) => Err(ClaimError::CoordinationRejected(format!(
            "push failed and the current remote claim could not be verified: {error}"
        ))),
    }
}

pub(super) fn take_over_stale(
    repo: &std::path::Path,
    task_uid: &str,
    expected_session: &str,
    base_commit: &str,
) -> Result<ClaimLease, ClaimError> {
    let reference = super::reference(task_uid)
        .map_err(|error| ClaimError::RemoteUnavailable(error.to_string()))?;
    let Some((record, old_object)) = inspect(repo, &reference)? else {
        return Err(ClaimError::RemoteUnavailable(
            "The claim no longer exists; retry normal acquisition".into(),
        ));
    };
    if record.session_id != expected_session || !record.appears_stale(now()) {
        return Err(ClaimError::AlreadyClaimed {
            stale: record.appears_stale(now()),
            record,
        });
    }
    let replacement = new_record(task_uid, base_commit);
    let object = create_object(repo, &replacement)?;
    let lease = format!("--force-with-lease={reference}:{old_object}");
    let refspec = format!("{object}:{reference}");
    let pushed = run(
        repo,
        &["push", "--porcelain", &lease, "origin", &refspec],
        None,
    )?;
    if pushed.status.success() {
        return Ok(ClaimLease {
            repo: repo.to_owned(),
            reference,
            object,
        });
    }
    if let Some((record, _)) = inspect(repo, &reference)? {
        return Err(ClaimError::AlreadyClaimed {
            stale: record.appears_stale(now()),
            record,
        });
    }
    Err(ClaimError::CoordinationRejected(
        String::from_utf8_lossy(&pushed.stderr).trim().to_owned(),
    ))
}

fn new_record(task_uid: &str, base_commit: &str) -> ClaimRecord {
    ClaimRecord {
        schema_version: 1,
        task_uid: task_uid.to_owned(),
        owner: "Kool.ad/e session".to_owned(),
        session_id: uuid::Uuid::new_v4().to_string(),
        base_commit: base_commit.to_owned(),
        claimed_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
    }
}

pub(super) fn create_object(
    repo: &std::path::Path,
    record: &ClaimRecord,
) -> Result<String, ClaimError> {
    let message = serde_json::to_vec(&record)
        .map_err(|error| ClaimError::RemoteUnavailable(error.to_string()))?;
    let tree = git(repo, &["mktree"], Some(b""))?;
    git(
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

fn inspect(
    repo: &std::path::Path,
    reference: &str,
) -> Result<Option<(ClaimRecord, String)>, ClaimError> {
    let listing = run(repo, &["ls-remote", "--refs", "origin", reference], None)?;
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
    let fetched = run(repo, &["fetch", "--no-tags", "origin", reference], None)?;
    if !fetched.status.success() {
        return Err(ClaimError::RemoteUnavailable(
            String::from_utf8_lossy(&fetched.stderr).trim().to_owned(),
        ));
    }
    let body = git(repo, &["show", "-s", "--format=%B", &object], None)?;
    serde_json::from_str(&body)
        .map(|record| Some((record, object)))
        .map_err(|error| {
            ClaimError::RemoteUnavailable(format!("Remote claim metadata is invalid: {error}"))
        })
}

pub(super) fn read_claim(
    repo: &std::path::Path,
    reference: &str,
    _object: &str,
) -> Result<ClaimRecord, ClaimError> {
    inspect(repo, reference)?.map(|pair| pair.0).ok_or_else(|| {
        ClaimError::RemoteUnavailable("Claim disappeared before it could be read".into())
    })
}

pub(super) fn refresh(
    repo: &std::path::Path,
    reference: &str,
    expected_object: &str,
) -> Result<String, ClaimError> {
    let Some((mut record, actual_object)) = inspect(repo, reference)? else {
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
    let pushed = run(
        repo,
        &["push", "--porcelain", &lease, "origin", &refspec],
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

pub(super) fn release(
    repo: &std::path::Path,
    reference: &str,
    object: &str,
) -> Result<(), ClaimError> {
    let listing = run(repo, &["ls-remote", "--refs", "origin", reference], None)?;
    if !listing.status.success() {
        return Err(ClaimError::RemoteUnavailable(
            String::from_utf8_lossy(&listing.stderr).trim().to_owned(),
        ));
    }
    if !String::from_utf8_lossy(&listing.stdout).starts_with(object) {
        return Ok(());
    }
    let lease = format!("--force-with-lease={reference}:{object}");
    let result = run(
        repo,
        &[
            "push",
            "--porcelain",
            &lease,
            "origin",
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

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
