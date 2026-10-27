use std::{
    ffi::CString,
    fs::File,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path},
};

#[cfg(target_os = "linux")]
fn same_device(stat_device: libc::dev_t, file_device: u64) -> bool {
    stat_device == file_device
}

#[cfg(not(target_os = "linux"))]
#[allow(clippy::useless_conversion)]
fn same_device(stat_device: libc::dev_t, file_device: u64) -> bool {
    u64::try_from(stat_device).ok() == Some(file_device)
}

pub(super) fn open_state_root(path: &Path, current_uid: u32) -> anyhow::Result<File> {
    anyhow::ensure!(
        path.is_absolute(),
        "Repair audit state root must be absolute"
    );
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::RootDir => None,
            Component::Normal(name) => Some(Ok(name)),
            _ => Some(Err(anyhow::anyhow!(
                "Repair audit state root contains an unsafe path component"
            ))),
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    anyhow::ensure!(
        !components.is_empty(),
        "Repair audit state root cannot be the filesystem root"
    );

    let root = CString::new("/")?;
    let root_fd = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    anyhow::ensure!(root_fd >= 0, "Filesystem root could not be opened safely");
    let mut current = unsafe { File::from_raw_fd(root_fd) };

    for (index, component) in components.iter().enumerate() {
        let name = CString::new(component.as_bytes())?;
        let mut fd = unsafe {
            libc::openat(
                current.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::NotFound || index + 1 != components.len() {
                return Err(error.into());
            }
            let created = unsafe { libc::mkdirat(current.as_raw_fd(), name.as_ptr(), 0o700) };
            anyhow::ensure!(
                created == 0
                    || std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists,
                "Repair audit state root could not be created safely"
            );
            fd = unsafe {
                libc::openat(
                    current.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
                )
            };
            anyhow::ensure!(
                fd >= 0,
                "Repair audit state root could not be opened safely"
            );
        }
        let next = unsafe { File::from_raw_fd(fd) };
        anyhow::ensure!(
            next.metadata()?.is_dir(),
            "Repair audit state root contains a non-directory component"
        );
        current = next;
    }
    anyhow::ensure!(
        current.metadata()?.uid() == current_uid,
        "Repair audit state root must be owned by the current user"
    );
    super::verify_macos_acl_absent(&current)?;
    Ok(current)
}

pub(super) fn verify_directory_matches_file(
    state_root: &File,
    name: &std::ffi::CStr,
    directory: &File,
    current_uid: u32,
) -> anyhow::Result<()> {
    let mut current = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            state_root.as_raw_fd(),
            name.as_ptr(),
            current.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    anyhow::ensure!(result == 0, "Repair audit directory identity changed");
    let current = unsafe { current.assume_init() };
    let opened = directory.metadata()?;
    anyhow::ensure!(
        current.st_mode & libc::S_IFMT == libc::S_IFDIR
            && current.st_uid == current_uid
            && same_device(current.st_dev, opened.dev())
            && current.st_ino == opened.ino(),
        "Repair audit directory identity changed"
    );
    Ok(())
}

pub(super) fn open_existing_audit(
    directory: &File,
    name: &std::ffi::CStr,
    current_uid: u32,
) -> anyhow::Result<Option<File>> {
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(error.into());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    verify_private_regular_file(&file, current_uid)?;
    super::secure_macos_acl(&file)?;
    anyhow::ensure!(
        file.metadata()?.len() <= super::super::MAX_AUDIT_BYTES,
        "Machine repair audit is oversized"
    );
    Ok(Some(file))
}

pub(super) fn verify_private_regular_file(file: &File, current_uid: u32) -> anyhow::Result<()> {
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == current_uid
            && metadata.nlink() == 1
            && metadata.mode() & 0o077 == 0,
        "Machine repair audit file must be private and singly linked"
    );
    Ok(())
}

pub(super) fn verify_path_matches_file(
    directory: &File,
    name: &std::ffi::CStr,
    file: &File,
    current_uid: u32,
) -> anyhow::Result<()> {
    let mut current = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            current.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    anyhow::ensure!(result == 0, "Repair audit lock identity changed");
    let current = unsafe { current.assume_init() };
    let opened = file.metadata()?;
    anyhow::ensure!(
        current.st_mode & libc::S_IFMT == libc::S_IFREG
            && current.st_uid == current_uid
            && current.st_nlink == 1
            && same_device(current.st_dev, opened.dev())
            && current.st_ino == opened.ino(),
        "Repair audit lock identity changed"
    );
    Ok(())
}
