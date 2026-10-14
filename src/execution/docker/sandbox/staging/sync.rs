use std::path::{Path, PathBuf};

use super::archive::{Fingerprint, Manifest, ReturnedEntry, parse_returned, validate_relative};

pub(super) fn apply(
    root: &Path,
    baseline: &Manifest,
    archive: &[u8],
    byte_limit: u64,
    entry_limit: usize,
) -> Result<(), String> {
    let returned = parse_returned(archive, byte_limit, entry_limit)
        .map_err(|_| "Container returned an invalid or oversized workspace archive")?;
    apply_entries(root, baseline, returned, byte_limit)
}

pub(super) fn apply_entries(
    root: &Path,
    baseline: &Manifest,
    returned: std::collections::BTreeMap<PathBuf, ReturnedEntry>,
    byte_limit: u64,
) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    return unix::apply(root, baseline, &returned, byte_limit);
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (root, baseline, returned);
        Err(
            "Safe task-clone synchronization is unavailable on this platform; host changes were preserved",
        )
    }
}

#[cfg(target_os = "linux")]
mod unix;
