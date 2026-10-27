#[cfg(target_os = "linux")]
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::{Read, Seek, Write},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

#[cfg(target_os = "linux")]
use sha2::Digest;

#[cfg(target_os = "linux")]
pub(super) fn validate_private_staging(path: &Path) -> anyhow::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o077 == 0
            && path.canonicalize()? == path,
        "Claude release staging directory is not private"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn install_verified_binary(
    source_path: &Path,
    target_path: &Path,
    expected_sha256: &str,
) -> anyhow::Result<()> {
    let expected_target =
        crate::app::bootstrap_remediation::claude_code_install::recipe::current_target()?;
    anyhow::ensure!(
        target_path == Path::new(&expected_target),
        "Claude install target differs from the app-owned current-user target"
    );
    let source = open_verified_staged_source(source_path, expected_sha256)?;
    let home = target_path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| anyhow::anyhow!("Claude install target has no user home directory"))?;
    let bin_directory = open_user_bin(home)?;
    atomic_install_at(&source, &bin_directory, "claude", expected_sha256)
}

#[cfg(target_os = "linux")]
fn open_verified_staged_source(path: &Path, expected: &str) -> anyhow::Result<File> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("verified Claude binary has no staging directory"))?;
    validate_private_staging(parent)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == unsafe { libc::geteuid() }
            && metadata.mode() & 0o022 == 0,
        "verified Claude binary staging file is not private"
    );
    anyhow::ensure!(
        hash_open_file(&file)? == expected,
        "verified Claude binary changed before staging"
    );
    Ok(file)
}

#[cfg(target_os = "linux")]
fn open_user_bin(home: &Path) -> anyhow::Result<File> {
    anyhow::ensure!(
        home.is_absolute() && home.canonicalize()? == home,
        "current user's home path is redirected"
    );
    let home_metadata = std::fs::symlink_metadata(home)?;
    let uid = unsafe { libc::geteuid() };
    anyhow::ensure!(
        home_metadata.is_dir()
            && !home_metadata.file_type().is_symlink()
            && home_metadata.uid() == uid
            && home_metadata.mode() & 0o022 == 0,
        "current user's home directory is not safely owned"
    );
    let home_path = CString::new(home.as_os_str().as_encoded_bytes())?;
    let home_fd = unsafe {
        libc::open(
            home_path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    anyhow::ensure!(
        home_fd >= 0,
        "could not open current user's home directory safely"
    );
    let home = unsafe { File::from_raw_fd(home_fd) };
    let local = open_or_create_directory(&home, ".local", uid)?;
    open_or_create_directory(&local, "bin", uid)
}

#[cfg(target_os = "linux")]
fn open_or_create_directory(parent: &File, name: &str, uid: u32) -> anyhow::Result<File> {
    let name = CString::new(name)?;
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    let mut fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if fd < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
        let created = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
        if created != 0
            && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
        {
            anyhow::bail!("could not create a private user-local directory");
        }
        fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
    }
    anyhow::ensure!(fd >= 0, "user-local directory is missing or redirected");
    let directory = unsafe { File::from_raw_fd(fd) };
    let metadata = directory.metadata()?;
    anyhow::ensure!(
        metadata.is_dir() && metadata.uid() == uid && metadata.mode() & 0o022 == 0,
        "user-local directory is not safely owned"
    );
    Ok(directory)
}

#[cfg(target_os = "linux")]
fn atomic_install_at(
    source: &File,
    directory: &File,
    target_name: &str,
    expected_sha256: &str,
) -> anyhow::Result<()> {
    let uid = unsafe { libc::geteuid() };
    let target = CString::new(target_name)?;
    if let Ok(existing) = open_at_readonly(directory, &target) {
        ensure_installed_file(&existing, expected_sha256, uid)?;
        return Ok(());
    }
    let staging_name = CString::new(format!(".claude-{}.stage", uuid::Uuid::new_v4()))?;
    let staging_fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            staging_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o700,
        )
    };
    anyhow::ensure!(
        staging_fd >= 0,
        "could not create a private Claude staging file"
    );
    let _staged_name = StagedName {
        directory,
        name: staging_name.clone(),
    };
    let mut staging_file = unsafe { File::from_raw_fd(staging_fd) };
    let mut source = source.try_clone()?;
    source.seek(std::io::SeekFrom::Start(0))?;
    let mut hasher = sha2::Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| anyhow::anyhow!("Claude binary size overflow"))?;
        anyhow::ensure!(
            total <= 512 * 1024 * 1024,
            "Claude binary exceeded its staging bound"
        );
        hasher.update(&buffer[..read]);
        staging_file.write_all(&buffer[..read])?;
    }
    anyhow::ensure!(
        format!("{:x}", hasher.finalize()) == expected_sha256,
        "Claude binary changed during user-local staging"
    );
    anyhow::ensure!(
        unsafe { libc::fchmod(staging_file.as_raw_fd(), 0o500) } == 0,
        "could not set safe permissions on the staged Claude binary"
    );
    staging_file.sync_all()?;
    let linked = unsafe {
        libc::linkat(
            directory.as_raw_fd(),
            staging_name.as_ptr(),
            directory.as_raw_fd(),
            target.as_ptr(),
            0,
        )
    };
    if linked != 0 {
        if std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists {
            let existing = open_at_readonly(directory, &target)?;
            ensure_installed_file(&existing, expected_sha256, uid)?;
            return Ok(());
        }
        anyhow::bail!("could not atomically publish the verified Claude binary");
    }
    directory.sync_all()?;
    let installed = open_at_readonly(directory, &target)?;
    ensure_installed_file(&installed, expected_sha256, uid)
}

#[cfg(target_os = "linux")]
fn ensure_installed_file(file: &File, expected: &str, uid: u32) -> anyhow::Result<()> {
    let metadata = file.metadata()?;
    anyhow::ensure!(
        metadata.is_file()
            && metadata.uid() == uid
            && metadata.mode() & 0o022 == 0
            && metadata.mode() & 0o111 != 0
            && hash_open_file(file)? == expected,
        "existing Claude install target is not the verified release"
    );
    Ok(())
}

#[cfg(target_os = "linux")]
fn open_at_readonly(directory: &File, name: &CString) -> anyhow::Result<File> {
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    anyhow::ensure!(
        fd >= 0,
        "Claude install target is unavailable or redirected"
    );
    Ok(unsafe { File::from_raw_fd(fd) })
}

#[cfg(target_os = "linux")]
fn unlink_at(directory: &File, name: &CString) {
    unsafe {
        libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0);
    }
}

#[cfg(target_os = "linux")]
struct StagedName<'a> {
    directory: &'a File,
    name: CString,
}

#[cfg(target_os = "linux")]
impl Drop for StagedName<'_> {
    fn drop(&mut self) {
        unlink_at(self.directory, &self.name);
    }
}

#[cfg(target_os = "linux")]
fn hash_open_file(file: &File) -> anyhow::Result<String> {
    use sha2::Digest;

    let mut copy = file.try_clone()?;
    copy.seek(std::io::SeekFrom::Start(0))?;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = copy.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(all(test, target_os = "linux"))]
#[path = "stage/tests.rs"]
mod tests;
