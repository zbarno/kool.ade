//! Bounded macOS QEMU child lifecycle primitives.
//!
//! Process groups and rlimits bound a trusted QEMU process; they are not a
//! sandbox and do not certify guest/host isolation or open the release gate.

use std::{
    io,
    os::unix::process::CommandExt,
    process::{Child, Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

const MAX_STOP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OPEN_FILES: libc::rlim_t = 256;

#[cfg(target_os = "macos")]
type ResourceLimitKind = libc::c_int;
#[cfg(not(target_os = "macos"))]
type ResourceLimitKind = libc::__rlimit_resource_t;

pub(crate) struct MacOsVmProcessV1 {
    child: Child,
}

impl MacOsVmProcessV1 {
    /// Start an absolute-path, already app-trusted QEMU executable in a
    /// private process group with per-process memory/CPU limits.
    pub(crate) fn spawn(
        mut command: Command,
        memory_bytes: u64,
        cpu_time_limit: Duration,
    ) -> io::Result<Self> {
        if !std::path::Path::new(command.get_program()).is_absolute()
            || memory_bytes == 0
            || cpu_time_limit.is_zero()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "macOS QEMU requires an absolute executable and nonzero resource limits",
            ));
        }
        let memory_limit = libc::rlimit {
            rlim_cur: memory_bytes,
            rlim_max: memory_bytes,
        };
        let cpu_seconds = cpu_time_limit
            .as_secs()
            .saturating_add(u64::from(cpu_time_limit.subsec_nanos() != 0));
        let cpu_limit = libc::rlimit {
            rlim_cur: cpu_seconds,
            rlim_max: cpu_seconds,
        };
        let no_core = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };

        command.env_clear();
        command.env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
        command.stdin(std::process::Stdio::null());
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        // SAFETY: this callback only invokes async-signal-safe POSIX calls and
        // captured POD limits; it runs between fork and exec.
        unsafe {
            command.pre_exec(move || {
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                set_limit(libc::RLIMIT_AS, &memory_limit)?;
                set_limit(libc::RLIMIT_CPU, &cpu_limit)?;
                set_limit(libc::RLIMIT_CORE, &no_core)?;
                let files = libc::rlimit {
                    rlim_cur: MAX_OPEN_FILES,
                    rlim_max: MAX_OPEN_FILES,
                };
                set_limit(libc::RLIMIT_NOFILE, &files)
            });
        }
        command.spawn().map(|child| Self { child })
    }

    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    pub(crate) fn terminate(&mut self, timeout: Duration) -> io::Result<ExitStatus> {
        if timeout.is_zero() || timeout > MAX_STOP_TIMEOUT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "macOS QEMU stop timeout is outside its bound",
            ));
        }
        let process_group = i32::try_from(self.child.id())
            .map_err(|_| io::Error::other("macOS QEMU pid is out of range"))?;
        signal_group(process_group, libc::SIGTERM)?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()?
                && !process_group_exists(process_group)
            {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                signal_group(process_group, libc::SIGKILL)?;
                return self.child.wait();
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for MacOsVmProcessV1 {
    fn drop(&mut self) {
        let Ok(process_group) = i32::try_from(self.child.id()) else {
            return;
        };
        // Closing the owning backend must not leave its QEMU process running.
        let _ = signal_group(process_group, libc::SIGKILL);
        let _ = self.child.wait();
    }
}

fn set_limit(resource: ResourceLimitKind, limit: &libc::rlimit) -> io::Result<()> {
    // SAFETY: the pointer references a live `rlimit` value for the duration of
    // the POSIX call, and the resource constants are supplied by libc.
    (unsafe { libc::setrlimit(resource, limit) } == 0)
        .then_some(())
        .ok_or_else(io::Error::last_os_error)
}

fn signal_group(process_group: i32, signal: libc::c_int) -> io::Result<()> {
    // SAFETY: a negative pid addresses only the child-created process group.
    (unsafe { libc::kill(-process_group, signal) } == 0)
        .then_some(())
        .ok_or_else(io::Error::last_os_error)
}

fn process_group_exists(process_group: i32) -> bool {
    // SAFETY: signal zero performs an existence/permission probe only.
    unsafe { libc::kill(-process_group, 0) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unbounded_commands_before_spawn() {
        let mut command = Command::new("/bin/sleep");
        assert!(MacOsVmProcessV1::spawn(command, 0, Duration::from_secs(1)).is_err());
        command = Command::new("sleep");
        assert!(MacOsVmProcessV1::spawn(command, 1 << 30, Duration::from_secs(1)).is_err());
    }

    #[test]
    fn terminates_the_owned_process_group_within_the_requested_bound() {
        let mut command = Command::new("/bin/sleep");
        command.arg("60");
        let mut process = MacOsVmProcessV1::spawn(command, 1 << 30, Duration::from_secs(60))
            .expect("spawn bounded test child");
        let pid = i32::try_from(process.id()).unwrap();
        let status = process
            .terminate(Duration::from_secs(2))
            .expect("stop bounded process group");
        assert!(!status.success());
        assert!(!process_group_exists(pid));
    }
}
