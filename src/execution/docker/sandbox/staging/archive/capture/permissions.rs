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
    let mut mode = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let mut mode = if directory { 0o755 } else { 0o644 };
    if directory {
        mode |= 0o700;
    } else {
        mode |= 0o200;
    }
    if read_only {
        mode &= !0o222;
    }
    mode
}
