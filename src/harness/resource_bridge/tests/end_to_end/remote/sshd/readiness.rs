use std::{
    fs,
    net::TcpStream,
    thread,
    time::{Duration, Instant},
};

use super::SshdFixture;

impl SshdFixture {
    pub(super) fn wait_ready(&mut self) -> std::io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait()? {
                let diagnostics = fs::read_to_string(self.root.join("sshd.log"))
                    .unwrap_or_else(|error| format!("could not read sshd log: {error}"));
                return Err(std::io::Error::other(format!(
                    "sshd exited before readiness with {status}; executable={} port={} host_key={} root={}; stderr={diagnostics:?}",
                    self.sshd.display(),
                    self.port,
                    self.host_key.display(),
                    self.root.display()
                )));
            }
            if TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "loopback sshd did not become ready",
                ));
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
