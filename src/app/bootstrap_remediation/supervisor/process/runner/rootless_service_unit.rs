#![cfg(target_os = "linux")]

//! Safely create the fixed per-user rootless Docker service unit.
//!
//! This helper only writes the unit file. It never invokes systemd or starts a
//! service. The caller must separately obtain consent and perform service
//! operations through the remediation supervisor.

use std::{
    ffi::CStr,
    fs::File,
    io::{Read, Write},
    os::fd::AsRawFd,
    path::Path,
};

const UNIT_NAME: &CStr = c"koolade-rootless-docker.service";
const UNIT_TEMPLATE: &[u8] = b"[Unit]\nDescription=Kool.ad/e rootless Docker service\n\n[Service]\nEnvironment=PATH=/usr/bin:/sbin:/usr/sbin:/bin\nExecStart=/usr/bin/dockerd-rootless.sh\nType=notify\nDelegate=yes\nRestart=always\n\n[Install]\nWantedBy=default.target\n";
const DIRECTORY_FLAGS: i32 =
    libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW;

mod inspect;
pub(super) use inspect::validate_existing_current_user_rootless_unit;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UnitFileOutcome {
    Created,
    AlreadyPresent,
}

pub(super) fn ensure_current_user_rootless_unit(home: &Path) -> anyhow::Result<UnitFileOutcome> {
    ensure_unit_for_uid(home, unsafe { libc::geteuid() })
}

fn ensure_unit_for_uid(home: &Path, current_uid: u32) -> anyhow::Result<UnitFileOutcome> {
    anyhow::ensure!(
        current_uid != 0,
        "rootless Docker service cannot be installed as root"
    );
    let home_directory = open_home_directory(home, current_uid)?;
    let config = open_or_create_directory(&home_directory, c".config", current_uid)?;
    let systemd = open_or_create_directory(&config, c"systemd", current_uid)?;
    let user = open_or_create_directory(&systemd, c"user", current_uid)?;
    ensure_unit_file(&user, current_uid)
}

fn open_home_directory(path: &Path, current_uid: u32) -> anyhow::Result<File> {
    super::home_chain::open_trusted_home(path, current_uid)
}

fn open_or_create_directory(parent: &File, name: &CStr, current_uid: u32) -> anyhow::Result<File> {
    use std::os::fd::FromRawFd;

    let mut fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), DIRECTORY_FLAGS) };
    if fd < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
        let created = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
        anyhow::ensure!(
            created == 0
                || std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists,
            "rootless Docker user-service directory could not be created safely"
        );
        fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), DIRECTORY_FLAGS) };
    }
    anyhow::ensure!(
        fd >= 0,
        "rootless Docker user-service directory is missing or unsafe"
    );
    let directory = unsafe { File::from_raw_fd(fd) };
    verify_directory(&directory, current_uid)?;
    verify_directory_entry(parent, name, &directory, current_uid)?;
    Ok(directory)
}

fn verify_directory(directory: &File, current_uid: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let metadata = directory.metadata()?;
    anyhow::ensure!(
        metadata.is_dir()
            && metadata.uid() == current_uid
            && metadata.mode() & 0o022 == 0
            && metadata.mode() & 0o700 == 0o700,
        "rootless Docker user-service directories must be current-user-owned and not group/world writable"
    );
    Ok(())
}

fn verify_directory_entry(
    parent: &File,
    name: &CStr,
    directory: &File,
    current_uid: u32,
) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let path_metadata = stat_at(parent, name)?;
    let opened = directory.metadata()?;
    anyhow::ensure!(
        path_metadata.st_mode & libc::S_IFMT == libc::S_IFDIR
            && path_metadata.st_uid == current_uid
            && path_metadata.st_dev == opened.dev() as libc::dev_t
            && path_metadata.st_ino == opened.ino(),
        "rootless Docker user-service directory changed during validation"
    );
    Ok(())
}

fn ensure_unit_file(directory: &File, current_uid: u32) -> anyhow::Result<UnitFileOutcome> {
    use std::os::fd::FromRawFd;

    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            UNIT_NAME.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            return existing_unit_file(directory, current_uid);
        }
        return Err(error.into());
    }

    let mut file = unsafe { File::from_raw_fd(fd) };
    let write_result = (|| -> anyhow::Result<()> {
        anyhow::ensure!(
            unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } == 0,
            "rootless Docker unit permissions could not be set safely"
        );
        verify_unit_file(&file, current_uid)?;
        file.write_all(UNIT_TEMPLATE)?;
        file.sync_all()?;
        verify_file_entry(directory, UNIT_NAME, &file, current_uid)?;
        Ok(())
    })();
    if let Err(error) = write_result {
        unlink_if_same_file(directory, UNIT_NAME, &file);
        return Err(error);
    }
    Ok(UnitFileOutcome::Created)
}

fn existing_unit_file(directory: &File, current_uid: u32) -> anyhow::Result<UnitFileOutcome> {
    use std::os::fd::FromRawFd;

    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            UNIT_NAME.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    anyhow::ensure!(
        fd >= 0,
        "rootless Docker unit path is not a regular app-managed file"
    );
    let mut file = unsafe { File::from_raw_fd(fd) };
    verify_unit_file(&file, current_uid)?;
    let mut contents = Vec::new();
    std::io::Read::by_ref(&mut file)
        .take((UNIT_TEMPLATE.len() + 1) as u64)
        .read_to_end(&mut contents)?;
    anyhow::ensure!(
        contents == UNIT_TEMPLATE,
        "rootless Docker unit conflicts with the fixed app-managed template"
    );
    verify_file_entry(directory, UNIT_NAME, &file, current_uid)?;
    Ok(UnitFileOutcome::AlreadyPresent)
}

fn verify_unit_file(file: &File, current_uid: u32) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == current_uid
            && metadata.nlink() == 1
            && metadata.mode() & 0o022 == 0
            && metadata.len() <= UNIT_TEMPLATE.len() as u64,
        "rootless Docker unit must be a current-user-owned, non-writable regular file"
    );
    Ok(())
}

fn verify_file_entry(
    directory: &File,
    name: &CStr,
    file: &File,
    current_uid: u32,
) -> anyhow::Result<()> {
    use std::os::unix::fs::MetadataExt;

    let path_metadata = stat_at(directory, name)?;
    let opened = file.metadata()?;
    anyhow::ensure!(
        path_metadata.st_mode & libc::S_IFMT == libc::S_IFREG
            && path_metadata.st_uid == current_uid
            && path_metadata.st_nlink == 1
            && path_metadata.st_dev == opened.dev() as libc::dev_t
            && path_metadata.st_ino == opened.ino(),
        "rootless Docker unit changed during validation"
    );
    Ok(())
}

fn stat_at(directory: &File, name: &CStr) -> anyhow::Result<libc::stat> {
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            directory.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    anyhow::ensure!(
        result == 0,
        "rootless Docker path changed during validation"
    );
    Ok(unsafe { stat.assume_init() })
}

fn unlink_if_same_file(directory: &File, name: &CStr, file: &File) {
    use std::os::unix::fs::MetadataExt;

    let Ok(path_metadata) = stat_at(directory, name) else {
        return;
    };
    let Ok(opened) = file.metadata() else {
        return;
    };
    if path_metadata.st_dev == opened.dev() as libc::dev_t
        && path_metadata.st_ino == opened.ino()
        && path_metadata.st_mode & libc::S_IFMT == libc::S_IFREG
    {
        unsafe {
            libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0);
        }
    }
}

#[cfg(test)]
#[path = "rootless_service_unit/tests.rs"]
mod tests;
