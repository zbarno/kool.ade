use fs2::FileExt;
use std::{
    fs,
    fs::OpenOptions,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    thread,
    time::Duration,
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
    worker_process_dir: Option<PathBuf>,
    worker_binary: Option<PathBuf>,
    worker_marker: Option<String>,
    pub(super) host_fingerprint: String,
}

impl SshdFixture {
    pub(super) fn start(root: &Path, worker_binary: &Path) -> std::io::Result<Self> {
        Self::start_with_worker_identity(root, worker_binary, None)
    }

    pub(super) fn start_for_worker(
        root: &Path,
        worker_binary: &Path,
        worker_id: &str,
    ) -> std::io::Result<Self> {
        Self::start_with_worker_identity(root, worker_binary, Some(worker_id))
    }

    fn start_with_worker_identity(
        root: &Path,
        worker_binary: &Path,
        worker_id: Option<&str>,
    ) -> std::io::Result<Self> {
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

        let worker_command = worker_state_command(root, worker_binary, worker_id)?;
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
                 AcceptEnv KOOLADE_HOME DOCKER_HOST XDG_RUNTIME_DIR\n\
                 ForceCommand {}\n",
                host_key.display(),
                root.join("sshd.pid").display(),
                authorized_keys.display(),
                worker_command,
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
        let mut child = ChildGuard(Some(command.spawn()?));

        let ssh_config = root.join("ssh_config");
        write_client_config(&ssh_config, port, &client_key)?;
        let ssh_wrapper = write_wrapper(root, "ssh-real", &ssh_config)?;
        let bad_ssh_config = root.join("ssh-bad-auth-config");
        write_client_config(&bad_ssh_config, port, &rejected_client_key)?;
        let unauthenticated_wrapper = write_wrapper(root, "ssh-bad-auth", &bad_ssh_config)?;

        let mut fixture = Self {
            child: child
                .0
                .take()
                .expect("sshd child is guarded until fixture setup"),
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
            worker_process_dir: worker_id.map(|_| root.join("worker-state/worker-processes")),
            worker_binary: worker_id
                .map(|_| worker_binary.canonicalize())
                .transpose()?,
            worker_marker: worker_id.map(str::to_owned),
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

    pub(super) fn terminate_worker_process_for_test(&self) -> std::io::Result<()> {
        let process_dir = self.worker_process_dir.as_ref().ok_or_else(|| {
            std::io::Error::other("worker process tracking is unavailable for this fixture")
        })?;
        let worker_binary = self.worker_binary.as_ref().ok_or_else(|| {
            std::io::Error::other("worker binary tracking is unavailable for this fixture")
        })?;
        let worker_marker = self.worker_marker.as_deref().ok_or_else(|| {
            std::io::Error::other("worker marker is unavailable for this fixture")
        })?;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let mut terminated = 0;
            for entry in fs::read_dir(process_dir)? {
                let entry = entry?;
                let contents = match fs::read_to_string(entry.path()) {
                    Ok(contents) => contents,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                };
                let Ok(pid) = contents.trim().parse::<libc::pid_t>() else {
                    continue;
                };
                if pid > 0 && process::terminate_worker_pid(pid, worker_binary, worker_marker)? {
                    terminated += 1;
                    let _ = fs::remove_file(entry.path());
                }
            }
            if terminated > 0 {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err(std::io::Error::other(
                    "worker fixture has no active worker server process",
                ));
            }
            thread::sleep(Duration::from_millis(10));
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
}

mod container_cleanup;
mod process;
mod readiness;
use guards::ChildGuard;
mod guards;
mod helpers;
pub(super) use helpers::*;
