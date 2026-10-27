use std::{
    ffi::CString,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::Path,
    sync::OnceLock,
    time::{Duration, Instant},
};

use fs2::FileExt;

#[cfg(target_os = "macos")]
const ACL_TYPE_EXTENDED: libc::c_int = 0x0000_0100;

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut std::ffi::c_void;
    fn acl_delete_fd_np(fd: libc::c_int, kind: libc::c_int) -> libc::c_int;
    fn acl_free(value: *mut std::ffi::c_void) -> libc::c_int;
}

mod paths;
use super::{AuditEntry, AuditStore, MAX_AUDIT_BYTES, MAX_EVENTS, VERSION};
use paths::{
    open_existing_audit, open_state_root, verify_directory_matches_file, verify_path_matches_file,
    verify_private_regular_file,
};

const LOCK_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn append(path: &Path, name: &str, entry: AuditEntry) -> anyhow::Result<()> {
    static PROCESS_LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    let _process_guard = PROCESS_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("Repair audit lock was poisoned"))?;

    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid repair audit directory"))?;
    let state_root = parent
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid repair audit state root"))?;
    let directory_component = parent
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Invalid repair audit directory name"))?;
    let directory_component_text = directory_component.to_string_lossy();
    anyhow::ensure!(
        !matches!(directory_component_text.as_ref(), "." | "..")
            && !directory_component_text.is_empty()
            && directory_component_text.len() <= 96
            && directory_component
                .as_bytes()
                .iter()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
        "Invalid repair audit directory name"
    );
    let current_uid = unsafe { libc::geteuid() };
    let state_root = open_state_root(state_root, current_uid)?;
    let directory_name = CString::new(directory_component.as_bytes())?;
    let created = unsafe { libc::mkdirat(state_root.as_raw_fd(), directory_name.as_ptr(), 0o700) };
    anyhow::ensure!(
        created == 0 || std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists,
        "Repair audit directory could not be created safely"
    );
    let directory_fd = unsafe {
        libc::openat(
            state_root.as_raw_fd(),
            directory_name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW,
        )
    };
    anyhow::ensure!(
        directory_fd >= 0,
        "Repair audit directory could not be opened safely"
    );
    let directory = unsafe { std::fs::File::from_raw_fd(directory_fd) };
    let directory_metadata = directory.metadata()?;
    verify_directory_matches_file(&state_root, &directory_name, &directory, current_uid)?;
    anyhow::ensure!(
        directory_metadata.is_dir() && directory_metadata.uid() == current_uid,
        "Repair audit directory must be owned by the current user"
    );
    if unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    anyhow::ensure!(
        directory.metadata()?.mode() & 0o077 == 0,
        "Repair audit directory permissions are not private"
    );
    secure_macos_acl(&directory)?;

    let lock_name = CString::new(format!("{name}.lock"))?;
    let lock_fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            lock_name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )
    };
    anyhow::ensure!(lock_fd >= 0, "Repair audit lock cannot be opened safely");
    let lock = unsafe { std::fs::File::from_raw_fd(lock_fd) };
    verify_private_regular_file(&lock, current_uid)?;
    if unsafe { libc::fchmod(lock.as_raw_fd(), 0o600) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    secure_macos_acl(&lock)?;
    verify_private_regular_file(&lock, current_uid)?;

    let deadline = Instant::now() + LOCK_TIMEOUT;
    loop {
        match lock.try_lock_exclusive() {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "Another application instance is recording a machine repair"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(error.into()),
        }
    }
    verify_directory_matches_file(&state_root, &directory_name, &directory, current_uid)?;
    verify_path_matches_file(&directory, &lock_name, &lock, current_uid)?;

    let audit_name = CString::new(name)?;
    let mut store = match open_existing_audit(&directory, &audit_name, current_uid)? {
        Some(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_AUDIT_BYTES + 1).read_to_end(&mut bytes)?;
            anyhow::ensure!(
                bytes.len() as u64 <= MAX_AUDIT_BYTES,
                "Machine repair audit is oversized"
            );
            let store: AuditStore = serde_json::from_slice(&bytes)?;
            anyhow::ensure!(
                store.schema_version == VERSION && store.entries.len() <= MAX_EVENTS,
                "Machine repair audit has an unsupported schema or size"
            );
            store
        }
        None => AuditStore {
            schema_version: VERSION,
            entries: Vec::new(),
        },
    };
    store.entries.push(entry);
    if store.entries.len() > MAX_EVENTS {
        store.entries.drain(..store.entries.len() - MAX_EVENTS);
    }
    let bytes = serde_json::to_vec(&store)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_AUDIT_BYTES,
        "Machine repair audit is oversized"
    );

    let temporary_name = CString::new(format!(".{name}.{}.tmp", uuid::Uuid::new_v4()))?;
    let temporary_fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            temporary_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )
    };
    anyhow::ensure!(
        temporary_fd >= 0,
        "Repair audit staging file cannot be created safely"
    );
    let mut temporary = unsafe { std::fs::File::from_raw_fd(temporary_fd) };
    let write_result = (|| -> anyhow::Result<()> {
        verify_private_regular_file(&temporary, current_uid)?;
        secure_macos_acl(&temporary)?;
        temporary.write_all(&bytes)?;
        temporary.sync_all()?;
        verify_directory_matches_file(&state_root, &directory_name, &directory, current_uid)?;
        verify_path_matches_file(&directory, &lock_name, &lock, current_uid)?;
        let renamed = unsafe {
            libc::renameat(
                directory.as_raw_fd(),
                temporary_name.as_ptr(),
                directory.as_raw_fd(),
                audit_name.as_ptr(),
            )
        };
        anyhow::ensure!(
            renamed == 0,
            "Repair audit could not be replaced atomically"
        );
        directory.sync_all()?;
        Ok(())
    })();
    if write_result.is_err() {
        unsafe {
            libc::unlinkat(directory.as_raw_fd(), temporary_name.as_ptr(), 0);
        }
    }
    write_result
}

#[cfg(target_os = "macos")]
pub(super) fn secure_macos_acl(file: &std::fs::File) -> anyhow::Result<()> {
    let existing = macos_acl(file)?;
    if existing.is_null() {
        return Ok(());
    }
    unsafe { acl_free(existing) };
    anyhow::ensure!(
        unsafe { acl_delete_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) } == 0,
        "Machine repair audit ACL could not be removed"
    );
    verify_macos_acl_absent(file)
}

#[cfg(target_os = "macos")]
pub(super) fn verify_macos_acl_absent(file: &std::fs::File) -> anyhow::Result<()> {
    let remaining = macos_acl(file)?;
    if !remaining.is_null() {
        unsafe { acl_free(remaining) };
        anyhow::bail!("Machine repair audit state has an extended ACL");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_acl(file: &std::fs::File) -> anyhow::Result<*mut std::ffi::c_void> {
    let existing = unsafe { acl_get_fd_np(file.as_raw_fd(), ACL_TYPE_EXTENDED) };
    if !existing.is_null() {
        return Ok(existing);
    }
    let error = std::io::Error::last_os_error();
    anyhow::ensure!(
        matches!(error.raw_os_error(), Some(libc::ENOENT | libc::EOPNOTSUPP)),
        "Machine repair audit ACL could not be inspected"
    );
    Ok(std::ptr::null_mut())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn secure_macos_acl(_file: &std::fs::File) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn verify_macos_acl_absent(_file: &std::fs::File) -> anyhow::Result<()> {
    Ok(())
}
