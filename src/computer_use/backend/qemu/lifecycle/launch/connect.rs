use std::{
    io::ErrorKind,
    os::unix::net::UnixStream,
    path::Path,
    thread,
    time::{Duration, Instant},
};

use crate::computer_use::ComputerBackendErrorV1 as Error;

const RETRY_INTERVAL: Duration = Duration::from_millis(50);

pub(super) fn connect_control_socket(path: &Path, timeout: Duration) -> Result<UnixStream, Error> {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return Ok(stream),
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::NotFound | ErrorKind::ConnectionRefused | ErrorKind::WouldBlock
                ) && Instant::now() < deadline =>
            {
                thread::sleep(
                    RETRY_INTERVAL.min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Err(_) => return Err(Error::GuestChannelUnavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::net::UnixListener, thread};

    #[test]
    fn retries_until_the_guest_creates_its_control_socket() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("guest-control.sock");
        let server_path = path.clone();
        let server = thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            let listener = UnixListener::bind(server_path).unwrap();
            listener.accept().unwrap();
        });

        let stream = connect_control_socket(&path, Duration::from_secs(2)).unwrap();
        drop(stream);
        server.join().unwrap();
    }

    #[test]
    fn stops_retrying_when_the_control_socket_deadline_expires() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("missing-control.sock");
        assert!(matches!(
            connect_control_socket(&path, Duration::from_millis(100)),
            Err(Error::GuestChannelUnavailable)
        ));
    }
}
