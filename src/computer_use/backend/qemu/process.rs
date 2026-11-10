use std::{
    fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

use super::super::{ComputerBackendErrorV1 as Error, registry::MachineOwnerRecordV1};

mod cgroup;
mod pid;
pub(super) use cgroup::CgroupV2;
use pid::pid_line;

const QEMU_SYSTEM: &str = "/usr/bin/qemu-system-x86_64";

pub(super) fn qemu_program() -> PathBuf {
    PathBuf::from(QEMU_SYSTEM)
}

pub(super) fn validate_qemu_program(path: &Path) -> Result<(), Error> {
    crate::computer_use::validate_trusted_system_executable(path)
        .map_err(|_| Error::QemuUnavailable)
}

pub(super) fn set_child_limits(
    command: &mut Command,
    max_file_bytes: u64,
    cgroup_procs: &Path,
    secret_fd: Option<libc::c_int>,
) -> Result<(), Error> {
    // The delegated cgroup bounds CPU, memory, and process count. RLIMIT_FSIZE
    // bounds the qcow2 overlay's maximum host allocation even if the guest
    // writes every sector. The QEMU process gets no inherited application env.
    use std::os::unix::ffi::OsStrExt;
    let cgroup_path = std::ffi::CString::new(cgroup_procs.as_os_str().as_bytes())
        .map_err(|_| Error::HostNotReady)?;
    unsafe {
        command.pre_exec(move || {
            libc::umask(0o077);
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if let Some(fd) = secret_fd {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            let limit = libc::rlimit {
                rlim_cur: max_file_bytes as libc::rlim_t,
                rlim_max: max_file_bytes as libc::rlim_t,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let no_core = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_CORE, &no_core) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let descriptor = libc::open(cgroup_path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
            if descriptor < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let (digits, count) = pid_line(libc::getpid() as u32);
            let written = libc::write(descriptor, digits.as_ptr().cast(), count);
            let _ = libc::close(descriptor);
            if written != count as isize {
                return Err(std::io::Error::last_os_error());
            }
            crate::computer_use::process_security::install_child_restrictions()
        });
    }
    Ok(())
}

pub(super) fn owned_process_start_ticks(
    pid: u32,
    owner: &MachineOwnerRecordV1,
) -> Result<u64, Error> {
    let start = proc_start_ticks(pid)?;
    if !identity_matches(pid, owner)? {
        return Err(Error::OwnershipMismatch);
    }
    Ok(start)
}

fn proc_start_ticks(pid: u32) -> Result<u64, Error> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::QemuExited
        } else {
            Error::Io
        }
    })?;
    // comm is parenthesized and may contain spaces; field 22 is starttime.
    let tail = stat
        .rsplit_once(") ")
        .map(|(_, tail)| tail)
        .ok_or(Error::OwnershipMismatch)?;
    tail.split_ascii_whitespace()
        .nth(19)
        .ok_or(Error::OwnershipMismatch)?
        .parse()
        .map_err(|_| Error::OwnershipMismatch)
}

fn proc_cmdline(pid: u32) -> Result<Vec<u8>, Error> {
    fs::read(format!("/proc/{pid}/cmdline")).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::QemuExited
        } else {
            Error::Io
        }
    })
}

fn process_label(owner: &MachineOwnerRecordV1) -> Vec<u8> {
    format!(
        "koolade:{}:{}:{}",
        owner.machine_id, owner.session_id, owner.lease_id
    )
    .into_bytes()
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn identity_matches(pid: u32, owner: &MachineOwnerRecordV1) -> Result<bool, Error> {
    let metadata = match fs::metadata(format!("/proc/{pid}")) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(Error::OwnershipMismatch),
    };
    use std::os::unix::fs::MetadataExt;
    if metadata.uid() != unsafe { libc::geteuid() } {
        return Err(Error::OwnershipMismatch);
    }
    let executable =
        fs::canonicalize(format!("/proc/{pid}/exe")).map_err(|_| Error::OwnershipMismatch)?;
    if executable != qemu_program() || unsafe { libc::getpgid(pid as i32) } != pid as i32 {
        return Err(Error::OwnershipMismatch);
    }
    Ok(contains_bytes(&proc_cmdline(pid)?, &process_label(owner)))
}

pub(super) fn is_process_alive(machine: &super::LiveMachine) -> Result<bool, Error> {
    let Some(pid) = machine.owner.pid else {
        return Ok(false);
    };
    let Some(expected) = machine.owner.process_start_ticks else {
        return Ok(false);
    };
    match proc_start_ticks(pid) {
        Ok(actual) if actual == expected => identity_matches(pid, &machine.owner),
        Ok(_) | Err(Error::QemuExited) => Ok(false),
        Err(error) => Err(error),
    }
}

pub(super) fn signal_owned(owner: &MachineOwnerRecordV1, signal: i32) -> Result<(), Error> {
    let pid = owner.pid.ok_or(Error::OwnershipMismatch)?;
    if Some(proc_start_ticks(pid)?) != owner.process_start_ticks || !identity_matches(pid, owner)? {
        return Err(Error::OwnershipMismatch);
    }
    if unsafe { libc::kill(-(pid as i32), signal) } != 0 {
        return Err(Error::QemuExited);
    }
    Ok(())
}

pub(super) fn terminate_owned(
    child: &mut Child,
    owner: &MachineOwnerRecordV1,
    timeout: &Duration,
) -> Result<(), Error> {
    if child.try_wait().map_err(|_| Error::Io)?.is_some() {
        return Ok(());
    }
    signal_owned(owner, libc::SIGTERM)?;
    let deadline = Instant::now() + *timeout;
    while Instant::now() < deadline {
        if child.try_wait().map_err(|_| Error::Io)?.is_some() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    signal_owned(owner, libc::SIGKILL)?;
    child.wait().map_err(|_| Error::Io)?;
    Ok(())
}

pub(super) fn terminate_recovered(
    pid: u32,
    owner: &MachineOwnerRecordV1,
    timeout: &Duration,
) -> Result<(), Error> {
    if owner.pid != Some(pid) || Some(proc_start_ticks(pid)?) != owner.process_start_ticks {
        return Err(Error::OwnershipMismatch);
    }
    signal_owned(owner, libc::SIGTERM)?;
    let deadline = Instant::now() + *timeout;
    while Instant::now() < deadline {
        if proc_start_ticks(pid).is_err() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    signal_owned(owner, libc::SIGKILL)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if proc_start_ticks(pid).is_err() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(25));
    }
    Err(Error::Io)
}

pub(super) fn find_orphaned_qemu(
    owner: &MachineOwnerRecordV1,
) -> Result<Option<(u32, u64)>, Error> {
    let entries = fs::read_dir("/proc").map_err(|_| Error::HostNotReady)?;
    let mut found = None;
    for entry in entries {
        let entry = entry.map_err(|_| Error::Io)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        let command_line = match proc_cmdline(pid) {
            Ok(command_line) => command_line,
            Err(Error::QemuExited) => continue,
            Err(error) => return Err(error),
        };
        if !contains_bytes(&command_line, &process_label(owner)) {
            continue;
        }
        if !identity_matches(pid, owner)? || found.is_some() {
            return Err(Error::OwnershipMismatch);
        }
        found = Some((pid, proc_start_ticks(pid)?));
    }
    Ok(found)
}
