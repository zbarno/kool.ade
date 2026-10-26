use std::path::Path;
#[cfg(unix)]
use std::{fs, io::Read};

#[cfg(unix)]
use tar::{Builder, EntryType, Header};

#[cfg(unix)]
mod diagnostics;
#[cfg(unix)]
mod filesystem;
#[cfg(unix)]
use diagnostics::io_failure;
#[cfg(unix)]
use filesystem::{open_root_nofollow, read_child_names};

#[cfg(unix)]
use super::super::archive::{Fingerprint, Manifest, hash, validate_relative};

pub(crate) fn capture(
    root: &Path,
    byte_limit: u64,
    read_only: bool,
    entry_limit: usize,
) -> anyhow::Result<super::super::WorkspaceSnapshot> {
    capture_excluding(root, byte_limit, read_only, entry_limit, &[])
}

pub(crate) fn capture_excluding(
    root: &Path,
    byte_limit: u64,
    read_only: bool,
    entry_limit: usize,
    excluded_paths: &[String],
) -> anyhow::Result<super::super::WorkspaceSnapshot> {
    #[cfg(not(unix))]
    {
        let _ = (root, byte_limit, read_only, entry_limit, excluded_paths);
        anyhow::bail!("Safe workspace staging is unavailable on this platform");
    }
    #[cfg(unix)]
    {
        let mut builder = Builder::new(Vec::new());
        let mut manifest = Manifest::new();
        let mut total_bytes = 0_u64;
        let root = open_root_nofollow(root)?;
        append_directory(
            &root,
            Path::new(""),
            &mut builder,
            &mut manifest,
            &mut total_bytes,
            byte_limit,
            entry_limit,
            read_only,
            excluded_paths,
        )?;
        let archive = builder
            .into_inner()
            .map_err(|error| io_failure("workspace archive could not be finalized", error))?;
        anyhow::ensure!(
            archive.len() as u64 <= byte_limit,
            "archive exceeds staging budget"
        );
        Ok(super::super::WorkspaceSnapshot {
            archive,
            manifest,
            scope_root: None,
        })
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg(unix)]
fn append_directory(
    directory: &fs::File,
    relative: &Path,
    builder: &mut Builder<Vec<u8>>,
    manifest: &mut Manifest,
    total_bytes: &mut u64,
    byte_limit: u64,
    entry_limit: usize,
    read_only: bool,
    excluded_paths: &[String],
) -> anyhow::Result<()> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    };
    let before = directory
        .metadata()
        .map_err(|error| io_failure("workspace directory metadata could not be read", error))?;
    let mut children = read_child_names(directory)?;
    children.sort();
    for name in children {
        let child_relative = relative.join(&name);
        if name == std::ffi::OsStr::new(".git") {
            continue;
        }
        validate_relative(&child_relative)?;
        if excluded_paths
            .iter()
            .any(|excluded| Path::new(excluded) == child_relative)
        {
            continue;
        }
        let child_name = std::ffi::CString::new(child_relative.file_name().unwrap().as_bytes())?;
        anyhow::ensure!(
            manifest.len() < entry_limit,
            "workspace has too many entries"
        );
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        let stat_result = unsafe {
            libc::fstatat(
                directory.as_raw_fd(),
                child_name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if stat_result != 0 {
            anyhow::bail!(
                "workspace entry could not be inspected safely: {:?}",
                std::io::Error::last_os_error().kind()
            );
        }
        let stat = unsafe { stat.assume_init() };
        let kind = stat.st_mode & libc::S_IFMT;
        anyhow::ensure!(kind != libc::S_IFLNK, "workspace contains a symlink");
        if kind == libc::S_IFDIR {
            let fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    child_name.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY,
                )
            };
            if fd < 0 {
                anyhow::bail!(
                    "workspace directory could not be opened safely: {:?}",
                    std::io::Error::last_os_error().kind()
                );
            }
            let child = unsafe { fs::File::from_raw_fd(fd) };
            let metadata = child.metadata().map_err(|error| {
                io_failure(
                    "workspace directory entry metadata could not be read",
                    error,
                )
            })?;
            let mode = mode_for(&metadata, true, read_only);
            append_header(builder, &child_relative, EntryType::Directory, 0, mode, &[])?;
            manifest.insert(child_relative.clone(), Fingerprint::Directory);
            append_directory(
                &child,
                &child_relative,
                builder,
                manifest,
                total_bytes,
                byte_limit,
                entry_limit,
                read_only,
                excluded_paths,
            )?;
        } else if kind == libc::S_IFREG {
            let fd = unsafe {
                libc::openat(
                    directory.as_raw_fd(),
                    child_name.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                )
            };
            if fd < 0 {
                anyhow::bail!(
                    "workspace file could not be opened safely: {:?}",
                    std::io::Error::last_os_error().kind()
                );
            }
            let mut file = unsafe { fs::File::from_raw_fd(fd) };
            let metadata = file
                .metadata()
                .map_err(|error| io_failure("workspace file metadata could not be read", error))?;
            anyhow::ensure!(
                metadata.is_file() && metadata.nlink() == 1,
                "workspace contains a hard link or special file"
            );
            *total_bytes = total_bytes.saturating_add(metadata.len());
            anyhow::ensure!(
                *total_bytes <= byte_limit,
                "workspace exceeds staging budget"
            );
            let mut bytes = Vec::with_capacity(metadata.len().min(1024 * 1024) as usize);
            (&mut file)
                .take(
                    byte_limit
                        .saturating_sub(*total_bytes)
                        .saturating_add(metadata.len())
                        .saturating_add(1),
                )
                .read_to_end(&mut bytes)
                .map_err(|error| io_failure("workspace file contents could not be read", error))?;
            let after = file.metadata().map_err(|error| {
                io_failure("workspace file metadata could not be rechecked", error)
            })?;
            anyhow::ensure!(
                bytes.len() as u64 == metadata.len()
                    && after.len() == metadata.len()
                    && after.ino() == metadata.ino()
                    && after.mtime() == metadata.mtime()
                    && after.mtime_nsec() == metadata.mtime_nsec()
                    && after.ctime() == metadata.ctime()
                    && after.ctime_nsec() == metadata.ctime_nsec()
                    && after.mode() == metadata.mode(),
                "workspace file changed while staging"
            );
            let digest = hash(&bytes);
            let mode = mode_for(&metadata, false, read_only);
            append_header(
                builder,
                &child_relative,
                EntryType::Regular,
                bytes.len() as u64,
                mode,
                &bytes,
            )?;
            manifest.insert(
                child_relative,
                Fingerprint::File {
                    sha256: digest,
                    size: bytes.len() as u64,
                    mode: source_mode(&metadata),
                },
            );
        } else {
            anyhow::bail!("workspace contains a special file");
        }
    }
    let after = directory.metadata().map_err(|error| {
        io_failure("workspace directory metadata could not be rechecked", error)
    })?;
    anyhow::ensure!(
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "workspace directory changed while staging"
    );
    Ok(())
}

#[cfg(unix)]
fn append_header(
    builder: &mut Builder<Vec<u8>>,
    path: &Path,
    kind: EntryType,
    size: u64,
    mode: u32,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let mut header = Header::new_gnu();
    header.set_entry_type(kind);
    header.set_size(size);
    header.set_mode(mode);
    header.set_uid(if mode & 0o200 != 0 { 10001 } else { 0 });
    header.set_gid(if mode & 0o200 != 0 { 10001 } else { 0 });
    header.set_mtime(0);
    header.set_cksum();
    builder
        .append_data(&mut header, path, bytes)
        .map_err(|error| io_failure("workspace archive entry could not be encoded", error))?;
    Ok(())
}

#[cfg(unix)]
mod permissions;
#[cfg(unix)]
use permissions::{mode_for, source_mode};
