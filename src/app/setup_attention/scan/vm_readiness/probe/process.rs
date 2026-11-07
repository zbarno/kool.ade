use std::{
    io::{self, Read},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use super::MAX_RETAINED_OUTPUT;

pub(super) fn bounded_command_output(
    program: &str,
    arguments: &[&str],
    timeout: Duration,
) -> io::Result<Option<(ExitStatus, Vec<u8>)>> {
    #[cfg(target_os = "linux")]
    {
        bounded_command_output_linux(program, arguments, timeout)
    }
    #[cfg(not(target_os = "linux"))]
    {
        bounded_command_output_threaded(program, arguments, timeout)
    }
}

#[cfg(target_os = "linux")]
fn bounded_command_output_linux(
    program: &str,
    arguments: &[&str],
    timeout: Duration,
) -> io::Result<Option<(ExitStatus, Vec<u8>)>> {
    use std::os::fd::AsRawFd;

    let mut command = Command::new(program);
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    // The filter prevents the probed binary and its descendants from escaping
    // the private process group before timeout/normal-exit cleanup.
    unsafe {
        command.pre_exec(crate::computer_use::process_security::install_child_restrictions);
    }
    let mut child = command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let Some(mut stdout) = child.stdout.take() else {
        kill_probe(&mut child);
        let _ = child.wait();
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "readiness probe stdout pipe was unavailable",
        ));
    };
    let fd = stdout.as_raw_fd();
    // SAFETY: fcntl only reads and updates flags on the owned pipe descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        let error = io::Error::last_os_error();
        kill_probe(&mut child);
        let _ = child.wait();
        return Err(error);
    }
    let deadline = Instant::now() + timeout;
    let mut leader_exited = false;
    let mut output = Vec::with_capacity(MAX_RETAINED_OUTPUT);
    let mut overflowed = false;
    let mut eof = false;
    let mut buffer = [0u8; 1024];
    loop {
        while !eof {
            if Instant::now() >= deadline {
                kill_probe(&mut child);
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "readiness probe timed out",
                ));
            }
            match stdout.read(&mut buffer) {
                Ok(0) => eof = true,
                Ok(read) => {
                    let remaining = MAX_RETAINED_OUTPUT.saturating_sub(output.len());
                    output.extend_from_slice(&buffer[..read.min(remaining)]);
                    overflowed |= read > remaining;
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) => {
                    kill_probe(&mut child);
                    let _ = child.wait();
                    return Err(error);
                }
            }
        }
        if !leader_exited {
            match probe_leader_exited_without_reaping(child.id()) {
                Ok(exited) => leader_exited = exited,
                Err(error) => {
                    kill_probe(&mut child);
                    let _ = child.wait();
                    return Err(error);
                }
            }
        }
        if leader_exited && eof {
            break;
        }
        if Instant::now() >= deadline {
            // Nonblocking reads keep the deadline even if an escaped descendant
            // inherits stdout after the process-group leader exits.
            kill_probe(&mut child);
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "readiness probe timed out",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    // Keep the leader unreaped until its private process group has been killed.
    // This also cleans helpers that closed stdout before the leader exited.
    kill_probe(&mut child);
    let status = child.wait()?;
    Ok((!overflowed).then_some((status, output)))
}

#[cfg(target_os = "linux")]
fn probe_leader_exited_without_reaping(pid: u32) -> io::Result<bool> {
    let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
    // SAFETY: waitid writes siginfo_t; WNOWAIT observes exit without reaping,
    // keeping the PID/PGID reserved until the caller kills the process group.
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            info.as_mut_ptr(),
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful waitid initialized siginfo_t, and si_pid reads that
    // initialized process identifier field.
    let info = unsafe { info.assume_init() };
    Ok(unsafe { info.si_pid() } != 0)
}

#[cfg(not(target_os = "linux"))]
fn bounded_command_output_threaded(
    program: &str,
    arguments: &[&str],
    timeout: Duration,
) -> io::Result<Option<(ExitStatus, Vec<u8>)>> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let Some(stdout) = child.stdout.take() else {
        kill_probe(&mut child);
        let _ = child.wait();
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "readiness probe stdout pipe was unavailable",
        ));
    };
    let reader = thread::Builder::new()
        .name("host-readiness-output".into())
        .spawn(move || retain_bounded(stdout))?;
    let deadline = Instant::now() + timeout;
    let mut status = None;
    loop {
        if status.is_none() {
            status = child.try_wait()?;
        }
        if status.is_some() && reader.is_finished() {
            break;
        }
        if Instant::now() >= deadline {
            kill_probe(&mut child);
            let _ = child.wait();
            let _ = reader.join();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "readiness probe timed out",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    let (output, overflowed) = reader
        .join()
        .map_err(|_| io::Error::other("readiness output reader failed"))??;
    Ok((!overflowed).then_some((status.expect("probe process exited"), output)))
}

fn kill_probe(child: &mut Child) {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: the child is placed in a dedicated process group immediately
        // before spawn, so this signal cannot target the parent process group.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = child.kill();
    }
}

#[cfg(any(not(target_os = "linux"), test))]
pub(super) fn retain_bounded(mut reader: impl Read) -> io::Result<(Vec<u8>, bool)> {
    let mut retained = Vec::with_capacity(MAX_RETAINED_OUTPUT);
    let mut overflowed = false;
    let mut buffer = [0u8; 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let remaining = MAX_RETAINED_OUTPUT.saturating_sub(retained.len());
        retained.extend_from_slice(&buffer[..read.min(remaining)]);
        overflowed |= read > remaining;
    }
    Ok((retained, overflowed))
}
