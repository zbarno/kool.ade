use std::fs;

pub(super) fn source_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    }
    #[cfg(not(unix))]
    {
        0o644
    }
}

pub(super) fn mode_for(metadata: &fs::Metadata, directory: bool, read_only: bool) -> u32 {
    #[cfg(unix)]
    let source_mode = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let source_mode = if directory { 0o755 } else { 0o644 };
    mode_for_bits(source_mode, directory, read_only)
}

fn mode_for_bits(source_mode: u32, directory: bool, read_only: bool) -> u32 {
    let mut mode = source_mode;
    if directory {
        mode |= 0o700;
    } else {
        // Workspace files are owned by the executor in the staged archive,
        // so grant owner access even when the source mode grants read access
        // only through the group or other permission bits.
        mode |= 0o600;
    }
    if read_only {
        mode &= !0o222;
    }
    mode
}

#[cfg(test)]
#[test]
fn staged_files_gain_owner_read_access_even_when_only_other_read_was_granted() {
    assert_eq!(mode_for_bits(0o004, false, false) & 0o600, 0o600);
}
