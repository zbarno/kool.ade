//! Bounded, operator-local records for coding-tool discovery.

use std::{
    fs::{self, OpenOptions},
    io::Read,
    path::PathBuf,
    sync::Mutex,
};

const LOG_NAME: &str = "harness-discovery.jsonl";
const LOCK_NAME: &str = "harness-discovery.lock";
const MAX_LOG_BYTES: usize = 256 * 1024;
static WRITE_LOCK: Mutex<()> = Mutex::new(());

#[derive(serde::Serialize)]
struct ProbeRecord<'a> {
    schema_version: u8,
    scan_id: u64,
    provider: &'a str,
    stage: &'static str,
    duration_ms: u64,
    outcome: &'static str,
    failure_code: Option<&'static str>,
}

fn log_path() -> PathBuf {
    diagnostics_dir().join(LOG_NAME)
}

fn diagnostics_dir() -> PathBuf {
    crate::persistence::state_root().join("diagnostics")
}

fn supported_provider(provider: &str) -> bool {
    matches!(
        provider,
        "pi" | "codex" | "claude" | "opencode" | "copilot" | "antigravity"
    )
}

fn retain_bounded(existing: &[u8], line: &[u8]) -> Vec<u8> {
    let keep_from = existing.len().saturating_sub(MAX_LOG_BYTES);
    let mut retained = existing[keep_from..].to_vec();
    if keep_from > 0
        && let Some(newline) = retained.iter().position(|byte| *byte == b'\n')
    {
        retained.drain(..=newline);
    }
    retained.extend_from_slice(line);
    if retained.len() > MAX_LOG_BYTES {
        let overflow = retained.len() - MAX_LOG_BYTES;
        if let Some(newline) = retained[overflow..].iter().position(|byte| *byte == b'\n') {
            retained.drain(..overflow + newline + 1);
        } else {
            retained.clear();
        }
    }
    retained
}

/// Append only bounded, non-sensitive probe metadata. Provider output,
/// executable paths, models, and authentication details are intentionally omitted.
pub fn record_probe(scan_id: u64, provider: &str, duration_ms: u64, ready: bool) {
    record_probe_result(scan_id, provider, duration_ms, ready, None, None);
}

/// Append typed stage and failure codes while omitting provider output and secrets.
pub fn record_probe_result(
    scan_id: u64,
    provider: &str,
    duration_ms: u64,
    ready: bool,
    stage: Option<&str>,
    failure_code: Option<&str>,
) {
    if !supported_provider(provider) {
        return;
    }
    let Ok(_guard) = WRITE_LOCK.lock() else {
        return;
    };
    let record = ProbeRecord {
        schema_version: 1,
        scan_id,
        provider,
        stage: supported_stage(stage).unwrap_or("provider_probe"),
        duration_ms,
        outcome: if ready { "ready" } else { "failed" },
        failure_code: logged_failure_code(ready, failure_code),
    };
    let Ok(mut line) = serde_json::to_vec(&record) else {
        return;
    };
    line.push(b'\n');
    if line.len() > 1024 {
        return;
    }
    let path = log_path();
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(parent) else {
        return;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).is_err() {
            return;
        }
    }
    let lock_path = parent.join(LOCK_NAME);
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let Ok(lock) = options.open(lock_path) else {
        return;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if lock
            .set_permissions(fs::Permissions::from_mode(0o600))
            .is_err()
        {
            return;
        }
    }
    if fs2::FileExt::lock_exclusive(&lock).is_err() {
        return;
    }
    let existing = fs::File::open(&path)
        .ok()
        .and_then(|file| {
            let mut bytes = Vec::with_capacity(MAX_LOG_BYTES + 1);
            file.take((MAX_LOG_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .ok()?;
            Some(bytes)
        })
        .unwrap_or_default();
    let retained = retain_bounded(&existing, &line);
    if crate::artifacts::atomic_write_bytes(&path, &retained).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
    }
    let _ = fs2::FileExt::unlock(&lock);
}

fn supported_stage(stage: Option<&str>) -> Option<&'static str> {
    match stage {
        Some("executable") => Some("executable"),
        Some("version") => Some("version"),
        Some("authentication") => Some("authentication"),
        Some("models") => Some("models"),
        Some("capability") => Some("capability"),
        _ => None,
    }
}

fn supported_failure_code(code: &str) -> Option<&'static str> {
    match code {
        "executable_missing" => Some("executable_missing"),
        "executable_invalid" => Some("executable_invalid"),
        "version_unavailable" => Some("version_unavailable"),
        "authentication_required" => Some("authentication_required"),
        "model_catalog_unavailable" => Some("model_catalog_unavailable"),
        "runtime_unavailable" => Some("runtime_unavailable"),
        "provider_probe_failed" => Some("provider_probe_failed"),
        "invalid_provider_result" => Some("invalid_provider_result"),
        _ => None,
    }
}

fn logged_failure_code(ready: bool, failure_code: Option<&str>) -> Option<&'static str> {
    failure_code
        .and_then(supported_failure_code)
        .or_else(|| (!ready).then_some("provider_unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_records_are_provider_allowlisted_and_do_not_include_probe_data() {
        let encoded = serde_json::to_string(&ProbeRecord {
            schema_version: 1,
            scan_id: 7,
            provider: "codex",
            stage: "provider_probe",
            duration_ms: 10,
            outcome: "failed",
            failure_code: Some("provider_unavailable"),
        })
        .unwrap();
        assert!(!encoded.contains("token"));
        assert!(!encoded.contains("/home/"));
        assert!(supported_provider("codex"));
        assert!(!supported_provider("unknown"));
        assert_eq!(supported_stage(Some("models")), Some("models"));
        assert_eq!(supported_stage(Some("private path")), None);
        assert_eq!(
            supported_failure_code("authentication_required"),
            Some("authentication_required")
        );
        assert_eq!(supported_failure_code("token=secret"), None);
        assert_eq!(
            logged_failure_code(true, Some("model_catalog_unavailable")),
            Some("model_catalog_unavailable")
        );
        assert_eq!(logged_failure_code(true, None), None);
        assert_eq!(
            logged_failure_code(false, None),
            Some("provider_unavailable")
        );
        let typed = ProbeRecord {
            schema_version: 1,
            scan_id: 8,
            provider: "codex",
            stage: supported_stage(Some("models")).unwrap(),
            duration_ms: 18,
            outcome: "failed",
            failure_code: supported_failure_code("model_catalog_unavailable"),
        };
        let encoded = serde_json::to_string(&typed).unwrap();
        assert!(encoded.contains("\"stage\":\"models\""));
        assert!(encoded.contains("\"failure_code\":\"model_catalog_unavailable\""));
    }

    #[test]
    fn retention_keeps_complete_recent_lines_within_the_size_limit() {
        let mut previous = vec![b'x'; MAX_LOG_BYTES];
        previous.extend_from_slice(b"\nold\n");
        let retained = retain_bounded(&previous, b"new\n");
        assert!(retained.len() <= MAX_LOG_BYTES);
        assert!(retained.ends_with(b"new\n"));
        assert_eq!(retained, b"old\nnew\n");
    }
}
