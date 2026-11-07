use std::{ffi::OsStr, path::Path, time::Duration};

#[cfg(unix)]
use std::{
    io::{self, Read},
    process::{Command, Stdio},
    time::Instant,
};

use super::{BoundedOutput, QemuProcessError};

#[cfg(unix)]
use super::MAX_CAPTURE_BYTES;

pub(super) fn run_bounded(
    path: &Path,
    args: &[&OsStr],
    timeout: Duration,
) -> Result<BoundedOutput, QemuProcessError> {
    #[cfg(unix)]
    {
        let mut command = Command::new(path);
        command
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        use std::os::unix::process::CommandExt;
        // Each tool invocation owns its process group so timeout kills helpers too.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                #[cfg(target_os = "linux")]
                {
                    crate::computer_use::process_security::install_child_restrictions()
                }
                #[cfg(not(target_os = "linux"))]
                {
                    Ok(())
                }
            });
        }
        let mut child = command.spawn().map_err(|_| QemuProcessError::Unavailable)?;
        let pid = child.id();
        let stdout_pipe = match child.stdout.take() {
            Some(pipe) => pipe,
            None => {
                kill_process_group(pid);
                let _ = child.wait();
                return Err(QemuProcessError::Failed);
            }
        };
        let mut stdout = match PipeCapture::new(stdout_pipe) {
            Ok(pipe) => pipe,
            Err(_) => {
                kill_process_group(pid);
                let _ = child.wait();
                return Err(QemuProcessError::Failed);
            }
        };
        let stderr_pipe = match child.stderr.take() {
            Some(pipe) => pipe,
            None => {
                kill_process_group(pid);
                let _ = child.wait();
                return Err(QemuProcessError::Failed);
            }
        };
        let mut stderr = match PipeCapture::new(stderr_pipe) {
            Ok(pipe) => pipe,
            Err(_) => {
                kill_process_group(pid);
                let _ = child.wait();
                return Err(QemuProcessError::Failed);
            }
        };
        let started = Instant::now();
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                kill_process_group(pid);
                let _ = child.wait();
                return Err(QemuProcessError::TimedOut);
            }
            if poll_pipes(&stdout, &stderr, remaining.min(Duration::from_millis(10))).is_err() {
                kill_process_group(pid);
                let _ = child.wait();
                return Err(QemuProcessError::Failed);
            }
            if stdout.drain().is_err() || stderr.drain().is_err() {
                kill_process_group(pid);
                let _ = child.wait();
                return Err(QemuProcessError::Failed);
            }
            match process_exited_without_reaping(pid) {
                Ok(true) if stdout.is_closed() && stderr.is_closed() => break,
                Ok(_) => {}
                Err(_) => {
                    kill_process_group(pid);
                    let _ = child.wait();
                    return Err(QemuProcessError::Failed);
                }
            }
        }
        // The leader remains an unreaped zombie while its private process group
        // is terminated, so its numeric PGID cannot be reused for another group.
        kill_process_group(pid);
        let status = child.wait().map_err(|_| QemuProcessError::Failed)?;
        if stdout.overflow || stderr.overflow {
            return Err(QemuProcessError::OutputTooLarge);
        }
        Ok(BoundedOutput {
            status,
            stdout: stdout.bytes,
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (path, args, timeout);
        Err(QemuProcessError::Unavailable)
    }
}

#[cfg(unix)]
struct PipeCapture<R> {
    reader: Option<R>,
    bytes: Vec<u8>,
    overflow: bool,
}

#[cfg(unix)]
impl<R: Read + std::os::fd::AsRawFd> PipeCapture<R> {
    fn new(reader: R) -> io::Result<Self> {
        let fd = reader.as_raw_fd();
        // SAFETY: fcntl operates on the live pipe descriptor owned by reader.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            reader: Some(reader),
            bytes: Vec::with_capacity(MAX_CAPTURE_BYTES),
            overflow: false,
        })
    }

    fn fd(&self) -> libc::c_int {
        use std::os::fd::AsRawFd;
        self.reader.as_ref().map_or(-1, AsRawFd::as_raw_fd)
    }

    fn is_closed(&self) -> bool {
        self.reader.is_none()
    }

    fn drain(&mut self) -> io::Result<()> {
        let mut buffer = [0_u8; 8192];
        loop {
            let Some(reader) = self.reader.as_mut() else {
                return Ok(());
            };
            match reader.read(&mut buffer) {
                Ok(0) => self.reader = None,
                Ok(count) => {
                    let available = MAX_CAPTURE_BYTES.saturating_sub(self.bytes.len());
                    let keep = count.min(available);
                    self.bytes.extend_from_slice(&buffer[..keep]);
                    self.overflow |= keep < count;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            }
            if self.reader.is_none() {
                return Ok(());
            }
        }
    }
}

#[cfg(unix)]
fn poll_pipes<R1: Read + std::os::fd::AsRawFd, R2: Read + std::os::fd::AsRawFd>(
    stdout: &PipeCapture<R1>,
    stderr: &PipeCapture<R2>,
    timeout: Duration,
) -> io::Result<()> {
    let mut descriptors = [
        libc::pollfd {
            fd: stdout.fd(),
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        },
        libc::pollfd {
            fd: stderr.fd(),
            events: libc::POLLIN | libc::POLLHUP,
            revents: 0,
        },
    ];
    let timeout_ms = timeout.as_millis().clamp(1, i32::MAX as u128) as i32;
    // SAFETY: descriptors points to two initialized pollfd values for live pipes.
    let result =
        unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, timeout_ms) };
    if result < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn process_exited_without_reaping(pid: u32) -> io::Result<bool> {
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    // SAFETY: waitid writes a siginfo_t and WNOWAIT leaves the child unreaped,
    // reserving its PID/PGID until cleanup can safely signal the private group.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: waitid succeeded and initialized the siginfo_t.
    // SAFETY: `si_pid` reads the siginfo pid member initialized by waitid.
    Ok(unsafe { info.assume_init().si_pid() } != 0)
}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    // SAFETY: the child was placed in a private process group with the same id.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}
