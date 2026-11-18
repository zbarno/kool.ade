use std::{
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::{Duration, Instant},
};

const MAX_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_QMP_LINE_BYTES: usize = 64 * 1024;
const MAX_UNMATCHED_QMP_MESSAGES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MacOsQmpControlCommandV1 {
    Pause,
    Resume,
    Shutdown,
}

impl MacOsQmpControlCommandV1 {
    fn qmp_name(self) -> &'static str {
        match self {
            Self::Pause => "stop",
            Self::Resume => "cont",
            Self::Shutdown => "system_powerdown",
        }
    }
}

pub(super) fn execute_control_command(
    socket_path: &Path,
    command: MacOsQmpControlCommandV1,
    timeout: Duration,
) -> io::Result<()> {
    if !socket_path.is_absolute() || timeout.is_zero() || timeout > MAX_COMMAND_TIMEOUT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "macOS QMP requires an absolute socket path and a bounded timeout",
        ));
    }
    let stream = UnixStream::connect(socket_path)?;
    stream.set_write_timeout(Some(timeout))?;
    execute_control_command_on_stream(stream, command, timeout)
}

fn execute_control_command_on_stream(
    stream: UnixStream,
    command: MacOsQmpControlCommandV1,
    timeout: Duration,
) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    let mut reader = BufReader::new(stream);
    let greeting = read_json_line(&mut reader, deadline)?;
    if greeting.get("QMP").is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "QEMU did not provide a QMP greeting",
        ));
    }
    execute(&mut reader, "qmp_capabilities", 1, deadline)?;
    execute(&mut reader, command.qmp_name(), 2, deadline)?;
    Ok(())
}

fn execute(
    reader: &mut BufReader<UnixStream>,
    command: &str,
    id: u8,
    deadline: Instant,
) -> io::Result<()> {
    let mut request = serde_json::json!({ "execute": command, "id": id }).to_string();
    request.push_str("\r\n");
    reader.get_mut().write_all(request.as_bytes())?;
    for _ in 0..MAX_UNMATCHED_QMP_MESSAGES {
        let response = read_json_line(reader, deadline)?;
        if response.get("id").and_then(serde_json::Value::as_u64) != Some(u64::from(id)) {
            continue;
        }
        if let Some(error) = response.get("error") {
            return Err(io::Error::other(format!("QMP {command} failed: {error}")));
        }
        if response.get("return").is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("QMP {command} response had no return value"),
            ));
        }
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("QMP {command} response was not received"),
    ))
}

fn read_json_line(
    reader: &mut BufReader<UnixStream>,
    deadline: Instant,
) -> io::Result<serde_json::Value> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "QMP command deadline elapsed",
        ));
    }
    reader.get_ref().set_read_timeout(Some(remaining))?;
    let mut line = String::new();
    let bytes = (&mut *reader)
        .take((MAX_QMP_LINE_BYTES + 1) as u64)
        .read_line(&mut line)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::WouldBlock && Instant::now() >= deadline {
                io::Error::new(io::ErrorKind::TimedOut, "QMP command deadline elapsed")
            } else {
                error
            }
        })?;
    if bytes == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "QMP closed before returning a response",
        ));
    }
    if bytes > MAX_QMP_LINE_BYTES || !line.ends_with('\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "QMP response exceeded the line limit or was incomplete",
        ));
    }
    serde_json::from_str(&line).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid QMP JSON: {error}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixStream,
        thread,
    };

    use super::*;

    fn qmp_fixture(
        expected_command: &'static str,
        control_error: Option<&'static str>,
    ) -> (UnixStream, thread::JoinHandle<()>) {
        let (client, server) = UnixStream::pair().expect("QMP socket pair");
        let thread = thread::spawn(move || {
            let mut server = BufReader::new(server);
            server
                .get_mut()
                .write_all(b"{\"QMP\":{\"version\":{}}}\r\n")
                .expect("write QMP greeting");

            let capabilities = read_request(&mut server);
            assert_eq!(capabilities["execute"], "qmp_capabilities");
            server
                .get_mut()
                .write_all(b"{\"return\":{},\"id\":1}\r\n")
                .expect("write capabilities response");

            let control = read_request(&mut server);
            assert_eq!(control["execute"], expected_command);
            let response = match control_error {
                Some(error) => serde_json::json!({ "error": { "desc": error }, "id": 2 }),
                None => serde_json::json!({ "return": {}, "id": 2 }),
            };
            writeln!(server.get_mut(), "{response}").expect("write control response");
        });
        (client, thread)
    }

    fn read_request(reader: &mut BufReader<UnixStream>) -> serde_json::Value {
        let mut line = String::new();
        reader.read_line(&mut line).expect("read QMP request");
        serde_json::from_str(&line).expect("parse QMP request")
    }

    #[test]
    fn fixed_lifecycle_commands_complete_qmp_handshake() {
        for (command, expected) in [
            (MacOsQmpControlCommandV1::Pause, "stop"),
            (MacOsQmpControlCommandV1::Resume, "cont"),
            (MacOsQmpControlCommandV1::Shutdown, "system_powerdown"),
        ] {
            let (stream, server) = qmp_fixture(expected, None);
            execute_control_command_on_stream(stream, command, Duration::from_secs(5))
                .expect("execute QMP command");
            server.join().expect("QMP fixture completed");
        }
    }

    #[test]
    fn qmp_rejections_and_invalid_greetings_fail_closed() {
        let (stream, server) = qmp_fixture("stop", Some("VM is shutting down"));
        assert!(
            execute_control_command_on_stream(
                stream,
                MacOsQmpControlCommandV1::Pause,
                Duration::from_secs(5),
            )
            .is_err()
        );
        server.join().expect("QMP fixture completed");

        let (client, mut server) = UnixStream::pair().expect("QMP socket pair");
        server
            .write_all(b"{\"notQMP\":true}\n")
            .expect("write invalid greeting");
        assert_eq!(
            execute_control_command_on_stream(
                client,
                MacOsQmpControlCommandV1::Pause,
                Duration::from_secs(5),
            )
            .expect_err("invalid QMP greeting rejected")
            .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn unmatched_qmp_events_cannot_extend_the_command_deadline() {
        let (client, server) = UnixStream::pair().expect("QMP socket pair");
        let server = thread::spawn(move || {
            let mut server = BufReader::new(server);
            server
                .get_mut()
                .write_all(b"{\"QMP\":{\"version\":{}}}\r\n")
                .expect("write QMP greeting");
            let _capabilities = read_request(&mut server);
            server
                .get_mut()
                .write_all(b"{\"return\":{},\"id\":1}\r\n")
                .expect("write capabilities response");
            let _control = read_request(&mut server);
            for _ in 0..100 {
                if writeln!(server.get_mut(), "{{\"event\":\"TICK\",\"id\":99}}").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
        });

        let started = Instant::now();
        let error = execute_control_command_on_stream(
            client,
            MacOsQmpControlCommandV1::Pause,
            Duration::from_millis(200),
        )
        .expect_err("unmatched events do not complete the control command");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_millis(800));
        server.join().expect("QMP event fixture completed");
    }

    #[test]
    fn qmp_endpoint_requires_an_absolute_path_and_bounded_timeout() {
        assert_eq!(
            execute_control_command(
                Path::new("relative.sock"),
                MacOsQmpControlCommandV1::Pause,
                Duration::from_secs(1),
            )
            .expect_err("relative QMP path rejected")
            .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            execute_control_command(
                Path::new("/private/session/qmp.sock"),
                MacOsQmpControlCommandV1::Pause,
                Duration::ZERO,
            )
            .expect_err("unbounded QMP timeout rejected")
            .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
