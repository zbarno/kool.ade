use std::{
    fs::File,
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::Path,
};

mod directory;
use directory::read_child_names;

#[cfg(target_os = "linux")]
fn same_device(stat_device: libc::dev_t, file_device: u64) -> bool {
    stat_device == file_device
}

#[cfg(not(target_os = "linux"))]
#[allow(clippy::useless_conversion)]
fn same_device(stat_device: libc::dev_t, file_device: u64) -> bool {
    u64::try_from(stat_device).ok() == Some(file_device)
}

pub(super) fn capture_files(
    root: &Path,
    byte_limit: u64,
    entry_limit: usize,
) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let root = open_root_nofollow(root)?;
    let mut files = Vec::new();
    let mut total_bytes = 0_u64;
    capture_directory(
        &root,
        Path::new(""),
        &mut files,
        &mut total_bytes,
        byte_limit,
        entry_limit,
    )?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

fn capture_directory(
    directory: &File,
    relative: &Path,
    files: &mut Vec<(String, Vec<u8>)>,
    total_bytes: &mut u64,
    byte_limit: u64,
    entry_limit: usize,
) -> anyhow::Result<()> {
    let before = directory.metadata()?;
    let mut names = read_child_names(directory)?;
    names.sort();
    for name in names {
        let child_relative = relative.join(&name);
        super::super::manifest::validate_relative(
            child_relative
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("cache path is not valid UTF-8"))?,
        )?;
        let name = std::ffi::CString::new(name.as_bytes())?;
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        anyhow::ensure!(
            unsafe {
                libc::fstatat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    stat.as_mut_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            } == 0,
            "cache entry changed while capturing"
        );
        let stat = unsafe { stat.assume_init() };
        match stat.st_mode & libc::S_IFMT {
            libc::S_IFDIR => {
                let child = open_directory(directory, &name, &stat)?;
                capture_directory(
                    &child,
                    &child_relative,
                    files,
                    total_bytes,
                    byte_limit,
                    entry_limit,
                )?;
            }
            libc::S_IFREG => {
                anyhow::ensure!(
                    files.len() < entry_limit,
                    "cache snapshot has too many files"
                );
                let mut file = open_regular_file(directory, &name, &stat)?;
                let metadata = file.metadata()?;
                anyhow::ensure!(
                    metadata.nlink() == 1,
                    "cache snapshot contains a hard-linked file"
                );
                *total_bytes = total_bytes.saturating_add(metadata.len());
                anyhow::ensure!(
                    *total_bytes <= byte_limit,
                    "cache snapshot exceeds its limit"
                );
                let mut bytes = Vec::with_capacity(metadata.len().min(1024 * 1024) as usize);
                (&mut file)
                    .take(metadata.len().saturating_add(1))
                    .read_to_end(&mut bytes)?;
                let after = file.metadata()?;
                anyhow::ensure!(
                    bytes.len() as u64 == metadata.len()
                        && after.dev() == metadata.dev()
                        && after.ino() == metadata.ino()
                        && after.len() == metadata.len()
                        && after.mtime() == metadata.mtime()
                        && after.mtime_nsec() == metadata.mtime_nsec()
                        && after.ctime() == metadata.ctime()
                        && after.ctime_nsec() == metadata.ctime_nsec(),
                    "cache file changed during snapshot capture"
                );
                files.push((
                    child_relative
                        .to_str()
                        .ok_or_else(|| anyhow::anyhow!("cache path is not valid UTF-8"))?
                        .to_owned(),
                    bytes,
                ));
            }
            _ => anyhow::bail!("cache snapshot contains a symlink or special file"),
        }
    }
    let after = directory.metadata()?;
    anyhow::ensure!(
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "cache directory changed during snapshot capture"
    );
    Ok(())
}

fn open_directory(
    parent: &File,
    name: &std::ffi::CString,
    expected: &libc::stat,
) -> anyhow::Result<File> {
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY
                | libc::O_CLOEXEC
                | libc::O_NOFOLLOW
                | libc::O_NONBLOCK
                | libc::O_DIRECTORY,
        )
    };
    anyhow::ensure!(descriptor >= 0, "cache directory changed or is unsafe");
    let directory = unsafe { File::from_raw_fd(descriptor) };
    let metadata = directory.metadata()?;
    anyhow::ensure!(
        metadata.is_dir()
            && same_device(expected.st_dev, metadata.dev())
            && metadata.ino() == expected.st_ino,
        "cache directory changed while capturing"
    );
    Ok(directory)
}

fn open_regular_file(
    parent: &File,
    name: &std::ffi::CString,
    expected: &libc::stat,
) -> anyhow::Result<File> {
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    anyhow::ensure!(descriptor >= 0, "cache file changed or is unsafe");
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file()
            && same_device(expected.st_dev, metadata.dev())
            && metadata.ino() == expected.st_ino,
        "cache entry changed to a special file"
    );
    Ok(file)
}

fn open_root_nofollow(path: &Path) -> anyhow::Result<File> {
    let mut current = File::open("/")?;
    for component in path.components() {
        match component {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => {
                let name = std::ffi::CString::new(name.as_bytes())?;
                current = open_directory(&current, &name, &directory_stat(&current, &name)?)
                    .map_err(|_| anyhow::anyhow!("cache root contains an unsafe path"))?;
            }
            _ => anyhow::bail!("cache snapshot root is not canonical"),
        }
    }
    Ok(current)
}

fn directory_stat(parent: &File, name: &std::ffi::CString) -> anyhow::Result<libc::stat> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    anyhow::ensure!(
        unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                stat.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } == 0,
        "cache root path is unavailable"
    );
    Ok(unsafe { stat.assume_init() })
}

#[cfg(test)]
mod tests {
    use super::{capture_files, directory_stat, open_regular_file, open_root_nofollow};
    use std::os::fd::AsRawFd;

    #[test]
    fn capture_rejects_a_symlinked_ancestor() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("cache");
        let outside = directory.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("entry"), b"outside bytes").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("nested")).unwrap();

        assert!(capture_files(&root, 1024, 16).is_err());
    }

    #[test]
    fn no_follow_open_rejects_a_fifo_without_waiting_for_a_writer() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("cache");
        std::fs::create_dir(&root).unwrap();
        let handle = open_root_nofollow(&root.canonicalize().unwrap()).unwrap();
        let name = std::ffi::CString::new("fifo").unwrap();
        assert!(unsafe { libc::mkfifoat(handle.as_raw_fd(), name.as_ptr(), 0o600) } == 0);
        let stat = directory_stat(&handle, &name).unwrap();
        assert!(open_regular_file(&handle, &name, &stat).is_err());
    }
}
