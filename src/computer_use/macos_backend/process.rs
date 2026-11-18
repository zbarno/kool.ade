//! Bounded macOS QEMU child lifecycle primitives.
//!
//! Process groups and CPU/file-size rlimits bound a trusted QEMU process; they are
//! not a sandbox and do not certify guest/host isolation or open the release
//! gate. Darwin rejects RLIMIT_AS, so the caller must bound guest RAM in the
//! QEMU launch configuration; this module cannot enforce a host memory cap.

#[path = "process/qemu_launch.rs"]
mod qemu_launch;

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
    process_group: Option<i32>,
}

impl MacOsVmProcessV1 {
    /// Start an absolute-path, already app-trusted QEMU executable in a
    /// private process group with a guest-RAM ceiling, CPU-time limit, and
    /// per-process file-size limit.
    pub(crate) fn spawn(
        mut command: Command,
        memory_bytes: u64,
        max_file_bytes: u64,
        cpu_time_limit: Duration,
    ) -> io::Result<Self> {
        if !std::path::Path::new(command.get_program()).is_absolute()
            || memory_bytes == 0
            || max_file_bytes == 0
            || cpu_time_limit.is_zero()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "macOS QEMU requires an absolute executable and nonzero resource limits",
            ));
        }
        #[cfg(not(target_os = "macos"))]
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
        let file_size_limit = libc::rlimit {
            rlim_cur: max_file_bytes as libc::rlim_t,
            rlim_max: max_file_bytes as libc::rlim_t,
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
                libc::umask(0o077);
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                #[cfg(not(target_os = "macos"))]
                set_limit(libc::RLIMIT_AS, &memory_limit)?;
                set_limit(libc::RLIMIT_CPU, &cpu_limit)?;
                set_limit(libc::RLIMIT_FSIZE, &file_size_limit)?;
                set_limit(libc::RLIMIT_CORE, &no_core)?;
                let files = libc::rlimit {
                    rlim_cur: MAX_OPEN_FILES,
                    rlim_max: MAX_OPEN_FILES,
                };
                set_limit(libc::RLIMIT_NOFILE, &files)
            });
        }
        command.spawn().map(|child| {
            let process_group = i32::try_from(child.id()).ok();
            Self {
                child,
                process_group,
            }
        })
    }

    pub(crate) fn id(&self) -> u32 {
        self.child.id()
    }

    pub(crate) fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        if status.is_some()
            && self
                .process_group
                .is_some_and(|process_group| !process_group_exists(process_group))
        {
            self.process_group = None;
        }
        Ok(status)
    }

    pub(crate) fn terminate(&mut self, timeout: Duration) -> io::Result<ExitStatus> {
        if timeout.is_zero() || timeout > MAX_STOP_TIMEOUT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "macOS QEMU stop timeout is outside its bound",
            ));
        }
        let Some(process_group) = self.process_group else {
            return self.child.wait();
        };
        if let Some(status) = self.child.try_wait()?
            && !process_group_exists(process_group)
        {
            self.process_group = None;
            return Ok(status);
        }
        signal_group(process_group, libc::SIGTERM)?;
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()?
                && !process_group_exists(process_group)
            {
                self.process_group = None;
                return Ok(status);
            }
            if Instant::now() >= deadline {
                match signal_group(process_group, libc::SIGKILL) {
                    Ok(()) => {}
                    Err(error) if error.raw_os_error() == Some(libc::ESRCH) => {}
                    Err(error) => return Err(error),
                }
                let status = self.child.wait()?;
                if process_group_exists(process_group) {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "macOS QEMU process group survived forced termination",
                    ));
                }
                self.process_group = None;
                return Ok(status);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for MacOsVmProcessV1 {
    fn drop(&mut self) {
        let Some(process_group) = self.process_group.take() else {
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
        assert!(MacOsVmProcessV1::spawn(command, 0, 1, Duration::from_secs(1)).is_err());
        command = Command::new("sleep");
        assert!(MacOsVmProcessV1::spawn(command, 1 << 30, 1, Duration::from_secs(1)).is_err());
    }

    /// Production boundary: the operating system enforces the inherited
    /// RLIMIT_FSIZE on the child and its descendants. A planner unit test
    /// cannot prove that the kernel rejects an oversized file write.
    /// Dependency/setup cost: local POSIX `/bin/dd`, under one second.
    #[test]
    fn qemu_file_size_limit_caps_child_output_at_the_session_disk_budget() {
        let directory = tempfile::tempdir().expect("temporary output directory");
        let output = directory.path().join("bounded-output.bin");
        let output_argument = format!("of={}", output.display());
        let mut command = Command::new("/bin/dd");
        command.args(["if=/dev/zero", &output_argument, "bs=8192", "count=1"]);
        let mut process = MacOsVmProcessV1::spawn(command, 1 << 30, 4096, Duration::from_secs(30))
            .expect("spawn child with a small file-size bound");
        let deadline = Instant::now() + Duration::from_secs(5);
        while process.try_wait().expect("poll bounded child").is_none() {
            assert!(
                Instant::now() < deadline,
                "child should hit its file-size limit"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(
            std::fs::metadata(output)
                .expect("bounded output exists")
                .len()
                <= 4096
        );
    }

    #[test]
    fn terminates_the_owned_process_group_within_the_requested_bound() {
        let mut command = Command::new("/bin/sleep");
        command.arg("60");
        let mut process =
            MacOsVmProcessV1::spawn(command, 1 << 30, 1 << 30, Duration::from_secs(60))
                .expect("spawn bounded test child");
        let pid = i32::try_from(process.id()).unwrap();
        let status = process
            .terminate(Duration::from_secs(2))
            .expect("stop bounded process group");
        assert!(!status.success());
        assert!(!process_group_exists(pid));
        assert_eq!(process.process_group, None);
    }

    #[test]
    fn drop_terminates_an_owned_process_group() {
        let mut command = Command::new("/bin/sleep");
        command.arg("60");
        let process = MacOsVmProcessV1::spawn(command, 1 << 30, 1 << 30, Duration::from_secs(60))
            .expect("spawn bounded test child");
        let pid = i32::try_from(process.id()).unwrap();
        drop(process);
        assert!(!process_group_exists(pid));
    }

    #[test]
    fn terminate_after_observing_natural_exit_is_idempotent() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 7"]);
        let mut process =
            MacOsVmProcessV1::spawn(command, 1 << 30, 1 << 30, Duration::from_secs(60))
                .expect("spawn bounded test child");
        let deadline = Instant::now() + Duration::from_secs(2);
        while process.try_wait().expect("observe test child").is_none() {
            assert!(Instant::now() < deadline, "test child should exit promptly");
            thread::sleep(Duration::from_millis(10));
        }
        let status = process
            .terminate(Duration::from_secs(2))
            .expect("terminate after natural exit");
        assert_eq!(status.code(), Some(7));
        assert_eq!(process.process_group, None);
    }
}
