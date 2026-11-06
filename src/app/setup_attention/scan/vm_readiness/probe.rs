use std::{
    io::{self, Read},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_RETAINED_OUTPUT: usize = 4096;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn qemu_output(arguments: &[&str]) -> io::Result<Option<Vec<u8>>> {
    let mut command = Command::new("/usr/bin/qemu-system-x86_64");
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
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
            "QEMU stdout pipe was unavailable",
        ));
    };
    let reader = match thread::Builder::new()
        .name("qemu-readiness-output".into())
        .spawn(move || retain_bounded(stdout))
    {
        Ok(reader) => reader,
        Err(error) => {
            kill_probe(&mut child);
            let _ = child.wait();
            return Err(error);
        }
    };
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                kill_probe(&mut child);
                let _ = child.wait();
                let _ = reader.join();
                return Err(error);
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "QEMU readiness probe timed out",
            ));
        }
        thread::sleep(Duration::from_millis(10));
    };
    let (output, overflowed) = reader
        .join()
        .map_err(|_| io::Error::other("QEMU output reader failed"))??;
    Ok((status.success() && !overflowed).then_some(output))
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

fn retain_bounded(mut reader: impl Read) -> io::Result<(Vec<u8>, bool)> {
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

#[cfg(test)]
#[path = "probe_tests.rs"]
mod tests;
