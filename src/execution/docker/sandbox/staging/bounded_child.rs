use std::{
    io::{self, Read},
    process::{Child, ExitStatus},
};

pub(crate) fn read_child_stdout_bounded(
    child: &mut Child,
    byte_limit: u64,
) -> io::Result<(Vec<u8>, ExitStatus)> {
    let Some(stdout) = child.stdout.take() else {
        let unavailable = io::Error::new(io::ErrorKind::BrokenPipe, "child stdout is unavailable");
        return match stop_and_reap(child) {
            Ok(_) => Err(unavailable),
            Err(error) => Err(error),
        };
    };
    let mut bytes = Vec::new();
    let read_result = stdout
        .take(byte_limit.saturating_add(1))
        .read_to_end(&mut bytes);
    let exceeded_limit = bytes.len() as u64 > byte_limit;
    let status = if exceeded_limit || read_result.is_err() {
        stop_and_reap(child)?
    } else {
        child.wait()?
    };
    read_result?;
    if exceeded_limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "child stdout exceeds byte limit",
        ));
    }
    Ok((bytes, status))
}

fn stop_and_reap(child: &mut Child) -> io::Result<ExitStatus> {
    let _ = child.kill();
    child.wait()
}

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        process::{Command, Stdio},
        thread,
        time::Duration,
    };

    use super::read_child_stdout_bounded;

    // Production boundary: the Git index readers cap and wait on a real child
    // process. A parser unit test cannot prove that a child blocked on a full
    // pipe is killed and reaped. The test respawns its own test executable, so
    // it has no external dependencies (no Docker/SSH) and trivial process setup.
    #[test]
    fn oversized_child_output_is_killed_and_reaped() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["child_flooder", "--nocapture"])
            .env("KOOLADE_BOUNDED_CHILD_FLOOD", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let error = read_child_stdout_bounded(&mut child, 32).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(child.try_wait().unwrap().is_some());
    }

    #[test]
    fn child_flooder() {
        if std::env::var_os("KOOLADE_BOUNDED_CHILD_FLOOD").is_none() {
            return;
        }
        let stdout = std::io::stdout();
        let mut stdout = stdout.lock();
        loop {
            if stdout.write_all(b"bounded-output-flood").is_err() {
                thread::sleep(Duration::from_secs(60));
                return;
            }
        }
    }
}
