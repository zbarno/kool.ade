use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::app::bootstrap_repair_policy::Operation;

const VERSION: u8 = 1;
const MAX_AUDIT_BYTES: u64 = 512 * 1024;
const MAX_EVENTS: usize = 256;

#[cfg(all(test, unix))]
mod tests;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AuditEntry {
    observed_at_unix_ms: u64,
    operation: Operation,
    prerequisite_id: String,
    proposal_sha256: String,
    binding_sha256: String,
    outcome: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct AuditStore {
    schema_version: u8,
    entries: Vec<AuditEntry>,
}

pub(super) fn append(
    path: &Path,
    operation: Operation,
    prerequisite_id: &str,
    proposal_digest: &str,
    binding_digest: &str,
    outcome: &str,
    observed_at_unix_ms: u64,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        prerequisite_id.starts_with("setup:")
            && prerequisite_id.len() <= 180
            && prerequisite_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b':' | b'-' | b'_')),
        "Invalid repair audit prerequisite"
    );
    anyhow::ensure!(
        is_sha256_hex(proposal_digest) && is_sha256_hex(binding_digest),
        "Invalid repair audit digest"
    );
    anyhow::ensure!(
        matches!(outcome, "approved" | "consumed" | "rejected" | "cancelled"),
        "Invalid repair audit outcome"
    );
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid repair audit file name"))?;
    anyhow::ensure!(
        !matches!(name, "." | "..")
            && !name.is_empty()
            && name.len() <= 96
            && name
                .bytes()
                .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') }),
        "Invalid repair audit file name"
    );
    #[cfg(any(unix, windows))]
    {
        let entry = AuditEntry {
            observed_at_unix_ms,
            operation,
            prerequisite_id: prerequisite_id.to_owned(),
            proposal_sha256: proposal_digest.to_owned(),
            binding_sha256: binding_digest.to_owned(),
            outcome: outcome.to_owned(),
        };
        #[cfg(unix)]
        return unix::append(path, name, entry);
        #[cfg(windows)]
        return windows::append(path, name, entry);
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (
            path,
            name,
            operation,
            prerequisite_id,
            proposal_digest,
            binding_digest,
            observed_at_unix_ms,
        );
        anyhow::bail!("Secure machine repair audit storage is unavailable on this platform")
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
