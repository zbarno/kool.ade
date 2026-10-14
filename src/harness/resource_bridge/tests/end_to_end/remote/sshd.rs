use fs2::FileExt;
use std::{
    fs,
    fs::OpenOptions,
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::execution::remote::ssh::{SshWorkerConfig, VerifiedSshWorker};

pub(super) struct SshdFixture {
    child: Child,
    root: PathBuf,
    sshd: PathBuf,
    username: String,
    port: u16,
    host_key: PathBuf,
    host_key_public: PathBuf,
    wrong_host_key_public: PathBuf,
    known_hosts: PathBuf,
    ssh_wrapper: PathBuf,
    unauthenticated_wrapper: PathBuf,
    pub(super) host_fingerprint: String,
}

impl SshdFixture {
    pub(super) fn start(root: &Path, worker_binary: &Path) -> std::io::Result<Self> {
        let sshd = find_sshd().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "sshd was not found")
        })?;
        let username = command_text("id", &["-un"])?;
        // nextest runs these integration tests in separate processes. Hold a
        // process-shared lock until sshd has bound the selected ephemeral port.
        let port_lock_path = std::env::temp_dir().join("koolade-sshd-fixture-port.lock");
        let port_lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(port_lock_path)?;
        port_lock.lock_exclusive()?;
        let port_listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = port_listener.local_addr()?.port();
        drop(port_listener);

        let host_key = root.join("sshd-host-key");
        let host_key_public = host_key.with_extension("pub");
        generate_key(&host_key)?;
        let client_key = root.join("sshd-client-key");
        let client_key_public = client_key.with_extension("pub");
        generate_key(&client_key)?;
        let rejected_client_key = root.join("sshd-rejected-client-key");
        generate_key(&rejected_client_key)?;
        let wrong_host_key = root.join("sshd-wrong-host-key");
        let wrong_host_key_public = wrong_host_key.with_extension("pub");
        generate_key(&wrong_host_key)?;
        if unsafe { libc::geteuid() } != 0 {
            let status = Command::new("sudo")
                .args(["-n", "chown", "root:root"])
                .arg(&host_key)
                .status()?;
            if !status.success() {
                return Err(std::io::Error::other(
                    "could not assign generated SSH host key to root for sshd",
                ));
            }
        }
        let authorized_keys = root.join("sshd-authorized-keys");
        fs::copy(&client_key_public, &authorized_keys)?;

        let known_hosts = root.join("known_hosts");
        let known_host_line = format!("fixture-worker {}\n", public_key_fields(&host_key_public)?);
        fs::write(&known_hosts, &known_host_line)?;
        let fingerprint_output = Command::new("ssh-keygen")
            .args(["-lf"])
            .arg(&host_key_public)
            .output()?;
        if !fingerprint_output.status.success() {
            return Err(std::io::Error::other(
                "could not fingerprint generated SSH host key",
            ));
        }
        let host_fingerprint = String::from_utf8_lossy(&fingerprint_output.stdout)
            .split_whitespace()
            .nth(1)
            .ok_or_else(|| std::io::Error::other("SSH host-key fingerprint is missing"))?
            .to_owned();

        let config = root.join("sshd_config");
        fs::write(
            &config,
            format!(
                "ListenAddress 127.0.0.1\n\
                 Port {port}\n\
                 LogLevel VERBOSE\n\
                 HostKey {}\n\
                 PidFile {}\n\
                 AuthorizedKeysFile {}\n\
                 StrictModes no\n\
                 PubkeyAuthentication yes\n\
                 AuthenticationMethods publickey\n\
                 PasswordAuthentication no\n\
                 KbdInteractiveAuthentication no\n\
                 PermitRootLogin yes\n\
                 UsePAM no\n\
                 AllowUsers {username}\n\
                 AcceptEnv KOOLADE_HOME DOCKER_HOST XDG_RUNTIME_DIR PATH\n\
                 ForceCommand {} worker serve --stdio\n",
                host_key.display(),
                root.join("sshd.pid").display(),
                authorized_keys.display(),
                shell_quote(&worker_binary.canonicalize()?.to_string_lossy()),
            ),
        )?;
        let mut command = if unsafe { libc::geteuid() } == 0 {
            Command::new(&sshd)
        } else {
            let mut elevated = Command::new("sudo");
            elevated.arg("-n").arg(&sshd);
            elevated
        };
        command
            .args(["-D", "-e", "-f"])
            .arg(&config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(fs::File::create(root.join("sshd.log"))?));
        let child = command.spawn()?;

        let ssh_config = root.join("ssh_config");
        write_client_config(&ssh_config, port, &client_key)?;
        let ssh_wrapper = write_wrapper(root, "ssh-real", &ssh_config)?;
        let bad_ssh_config = root.join("ssh-bad-auth-config");
        write_client_config(&bad_ssh_config, port, &rejected_client_key)?;
        let unauthenticated_wrapper = write_wrapper(root, "ssh-bad-auth", &bad_ssh_config)?;

        let mut fixture = Self {
            child,
            root: root.to_owned(),
            sshd,
            username,
            port,
            host_key,
            host_key_public,
            wrong_host_key_public,
            known_hosts,
            ssh_wrapper,
            unauthenticated_wrapper,
            host_fingerprint,
        };
        fixture.wait_ready()?;
        FileExt::unlock(&port_lock)?;
        Ok(fixture)
    }

    pub(super) fn worker_config(&self, worker_id: &str) -> SshWorkerConfig {
        SshWorkerConfig {
            stable_id: worker_id.to_owned(),
            host_alias: "fixture-host".into(),
            ssh_principal: self.username.clone(),
            verified_host_key_fingerprint: self.host_fingerprint.clone(),
            known_hosts_path: self.known_hosts.clone(),
            ssh_binary: self.ssh_wrapper.clone(),
        }
    }

    pub(super) fn unauthenticated_worker(&self, worker: &SshWorkerConfig) -> VerifiedSshWorker {
        let mut bad_auth = worker.clone();
        bad_auth
            .ssh_binary
            .clone_from(&self.unauthenticated_wrapper);
        bad_auth
            .verify_known_host()
            .expect("known host should still verify")
    }

    pub(super) fn run_worker_command(&self, worker: &VerifiedSshWorker) -> Output {
        let argv = worker.argv();
        Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .expect("real SSH client should start")
    }

    pub(super) fn verify_client_authentication(&self) -> std::io::Result<()> {
        let output = Command::new(&self.ssh_wrapper)
            .args(["fixture-host", "true"])
            .output()?;
        if output.status.success() {
            return Ok(());
        }
        let daemon_log = fs::read_to_string(self.root.join("sshd.log"))
            .unwrap_or_else(|error| format!("could not read sshd log: {error}"));
        Err(std::io::Error::other(format!(
            "fixture SSH authentication failed: client_stderr={:?}; server_log={:?}",
            String::from_utf8_lossy(&output.stderr),
            daemon_log.lines().rev().take(20).collect::<Vec<_>>()
        )))
    }

    pub(super) fn write_wrong_known_host(&self, worker_id: &str) -> std::io::Result<()> {
        fs::write(
            &self.known_hosts,
            format!(
                "{worker_id} {}\n",
                public_key_fields(&self.wrong_host_key_public)?
            ),
        )
    }

    pub(super) fn restore_known_host(&self, worker_id: &str) -> std::io::Result<()> {
        fs::write(
            &self.known_hosts,
            format!(
                "{worker_id} {}\n",
                public_key_fields(&self.host_key_public)?
            ),
        )
    }

    fn wait_ready(&mut self) -> std::io::Result<()> {
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

impl Drop for SshdFixture {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

mod helpers;
pub(super) use helpers::*;
