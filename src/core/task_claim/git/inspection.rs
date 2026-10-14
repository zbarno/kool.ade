use std::{
    fs::OpenOptions,
    path::Path,
    time::{Duration, Instant},
};

use super::{
    ClaimError, ClaimRecord, MAX_REMOTE_CLAIM_BYTES, git, run, validate_remote,
    validate_remote_record,
};

pub(in crate::core::task_claim) fn inspect(
    repo: &std::path::Path,
    remote: &str,
    reference: &str,
) -> Result<Option<(ClaimRecord, String)>, ClaimError> {
    let _inspection_lock = lock_checkout(repo)?;
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
    let filter_args = [
        "fetch",
        "--dry-run",
        "--no-tags",
        "--no-write-fetch-head",
        "--filter=tree:0",
        "--depth=1",
        remote,
        reference,
    ];
    let preflight = run(repo, &filter_args, None)?;
    if !preflight.status.success()
        || String::from_utf8_lossy(&preflight.stderr)
            .to_ascii_lowercase()
            .contains("filtering not recognized")
    {
        return Err(ClaimError::CoordinationRejected(
            "Planning remote does not support bounded task-claim fetches".into(),
        ));
    }
    let fetched = run(
        repo,
        &[
            "fetch",
            "--no-tags",
            "--no-write-fetch-head",
            "--filter=tree:0",
            "--depth=1",
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
    if !matches!(object.len(), 40 | 64) || !object.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ClaimError::CoordinationRejected(
            "Planning remote claim object ID is invalid".into(),
        ));
    }
    let size = git(repo, &["cat-file", "-s", &object], None)?
        .parse::<usize>()
        .map_err(|_| ClaimError::CoordinationRejected("Planning claim size is invalid".into()))?;
    if size > MAX_REMOTE_CLAIM_BYTES {
        return Err(ClaimError::CoordinationRejected(
            "Planning remote claim exceeds the 64 KiB size limit".into(),
        ));
    }
    let body = git(repo, &["show", "-s", "--format=%B", &object], None)?;
    if body.len() > MAX_REMOTE_CLAIM_BYTES {
        return Err(ClaimError::CoordinationRejected(
            "Planning remote claim metadata exceeds the 64 KiB size limit".into(),
        ));
    }
    let record: ClaimRecord = serde_json::from_str(&body).map_err(|error| {
        ClaimError::CoordinationRejected(format!(
            "Planning remote claim metadata is invalid: {error}"
        ))
    })?;
    validate_remote_record(&record)?;
    Ok(Some((record, object)))
}

fn lock_checkout(repo: &Path) -> Result<std::fs::File, ClaimError> {
    let common = git(
        repo,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        None,
    )?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(Path::new(&common).join("koolade-claim-inspection.lock"))
        .map_err(|error| {
            ClaimError::RemoteUnavailable(format!("Claim inspection lock is unavailable: {error}"))
        })?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match fs2::FileExt::try_lock_exclusive(&file) {
            Ok(()) => return Ok(file),
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => {
                return Err(ClaimError::RemoteUnavailable(
                    "Claim inspection lock could not be acquired within its deadline".into(),
                ));
            }
        }
    }
}
