use std::path::Path;

#[cfg(any(target_os = "linux", windows))]
pub(super) fn platform_execution_supported() -> bool {
    true
}

#[cfg(not(any(target_os = "linux", windows)))]
pub(super) fn platform_execution_supported() -> bool {
    false
}

pub(super) fn validate_program_path(path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        path.is_absolute(),
        "trusted remediation program is not absolute"
    );
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "trusted remediation program is not a regular file"
    );
    #[cfg(all(unix, not(test)))]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            metadata.uid() == 0 && metadata.mode() & 0o022 == 0,
            "trusted remediation program is writable by an untrusted user"
        );
        let canonical = path.canonicalize()?;
        anyhow::ensure!(
            canonical == path,
            "trusted remediation program path changed during validation"
        );
    }
    #[cfg(all(unix, test))]
    anyhow::ensure!(
        path.canonicalize()? == path,
        "test remediation executable path changed"
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        anyhow::ensure!(
            metadata.file_attributes()
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
                == 0,
            "trusted remediation program is a reparse point"
        );
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn install_linux_process_group_guard() -> std::io::Result<()> {
    use std::io::Error;

    const BPF_LD_W_ABS: u16 = 0x20;
    const BPF_JMP_JEQ_K: u16 = 0x15;
    const BPF_RET_K: u16 = 0x06;
    const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
    const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
    const EPERM: u32 = libc::EPERM as u32;

    // Deny new sessions and process-group changes so descendants remain inside
    // the process group the supervisor terminates on timeout or cancellation.
    let filters = [
        libc::sock_filter {
            code: BPF_LD_W_ABS,
            jt: 0,
            jf: 0,
            k: 0,
        },
        libc::sock_filter {
            code: BPF_JMP_JEQ_K,
            jt: 0,
            jf: 1,
            k: libc::SYS_setsid as u32,
        },
        libc::sock_filter {
            code: BPF_RET_K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ERRNO | EPERM,
        },
        libc::sock_filter {
            code: BPF_JMP_JEQ_K,
            jt: 0,
            jf: 1,
            k: libc::SYS_setpgid as u32,
        },
        libc::sock_filter {
            code: BPF_RET_K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ERRNO | EPERM,
        },
        libc::sock_filter {
            code: BPF_RET_K,
            jt: 0,
            jf: 0,
            k: SECCOMP_RET_ALLOW,
        },
    ];
    let program = libc::sock_fprog {
        len: filters.len() as u16,
        filter: filters.as_ptr().cast_mut(),
    };
    if unsafe { libc::setpgid(0, 0) } != 0
        || unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0
        || unsafe {
            libc::prctl(
                libc::PR_SET_SECCOMP,
                libc::SECCOMP_MODE_FILTER,
                &program as *const libc::sock_fprog,
            )
        } != 0
    {
        return Err(Error::other(
            "could not install remediation process containment",
        ));
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn make_artifacts_inheritable(
    descriptors: &[std::os::fd::RawFd],
) -> std::io::Result<()> {
    use std::io::Error;

    for descriptor in descriptors {
        let flags = unsafe { libc::fcntl(*descriptor, libc::F_GETFD) };
        if flags < 0
            || unsafe { libc::fcntl(*descriptor, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0
        {
            return Err(Error::other(
                "could not pass the immutable installer artifact to its trusted consumer",
            ));
        }
    }
    Ok(())
}
