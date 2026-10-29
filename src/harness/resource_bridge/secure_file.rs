use std::{fs, io, path::Path};

/// Open a path without following a symbolic link or reparse point.
///
/// Unix lockfile readers use `O_NOFOLLOW` and `O_NONBLOCK` so a replaced path
/// cannot redirect the read or block on a FIFO. The Windows resource broker is
/// unavailable, but shared lockfile readers still need to reject reparse points.
pub(super) fn open_read_nofollow(path: &Path) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    configure_no_follow(&mut options);
    options.open(path)
}

#[cfg(unix)]
fn configure_no_follow(options: &mut fs::OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK);
}

#[cfg(windows)]
fn configure_no_follow(options: &mut fs::OpenOptions) {
    use std::os::windows::fs::OpenOptionsExt;
    // FILE_FLAG_OPEN_REPARSE_POINT: open the reparse point itself so the caller
    // can reject it using the opened handle's metadata.
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
}

#[cfg(not(any(unix, windows)))]
fn configure_no_follow(_options: &mut fs::OpenOptions) {}
