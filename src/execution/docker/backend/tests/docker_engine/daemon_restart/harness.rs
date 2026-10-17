use std::{
    fs,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    process::{Child, Command, Stdio},
};

pub(super) struct IsolatedRootlessDaemon {
    root: std::path::PathBuf,
    runtime: std::path::PathBuf,
    socket: std::path::PathBuf,
    process: Option<RootlessProcess>,
}

impl IsolatedRootlessDaemon {
    pub(super) async fn start(fixture_root: &std::path::Path) -> Self {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let root = fixture_root.join(format!("isolated-rootless-{id}"));
        for path in ["home", "tmp", "data", "exec"] {
            fs::create_dir_all(root.join(path)).expect("create private daemon directory");
        }
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .expect("restrict private daemon directory");
        let runtime = std::env::temp_dir().join(format!("koolade-runtime-{id}"));
        fs::create_dir_all(&runtime).expect("create private daemon runtime directory");
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700))
            .expect("restrict private daemon runtime directory");
        let socket = std::env::temp_dir().join(format!("koolade-docker-{id}.sock"));
        let mut daemon = Self {
            root,
            runtime,
            socket,
            process: None,
        };
        daemon.launch().await;
        daemon
    }

    pub(super) fn endpoint(&self) -> crate::execution::docker::engine::DockerEndpoint {
        crate::execution::docker::engine::DockerEndpoint::Unix {
            path: self.socket.clone(),
            desktop: false,
        }
    }

    async fn launch(&mut self) {
        let host_runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
            .expect("CI rootless Docker session must set XDG_RUNTIME_DIR");
        let dbus_address = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").unwrap_or_else(|| {
            format!(
                "unix:path={}/bus",
                std::path::PathBuf::from(&host_runtime_dir).display()
            )
            .into()
        });
        let log =
            fs::File::create(self.root.join("dockerd.log")).expect("create private daemon log");
        let child = Command::new("dockerd-rootless.sh")
            .arg(format!("--host=unix://{}", self.socket.display()))
            .arg(format!("--data-root={}", self.root.join("data").display()))
            .arg(format!("--exec-root={}", self.root.join("exec").display()))
            .arg(format!(
                "--pidfile={}",
                self.root.join("dockerd.pid").display()
            ))
            .env("HOME", self.root.join("home"))
            .env("TMPDIR", self.root.join("tmp"))
            .env("XDG_RUNTIME_DIR", &self.runtime)
            .env("DBUS_SESSION_BUS_ADDRESS", dbus_address)
            .env(
                "DOCKERD_ROOTLESS_ROOTLESSKIT_STATE_DIR",
                self.root.join("rootlesskit"),
            )
            .env(
                "CONTAINERD_ROOTLESS_ROOTLESSKIT_STATE_DIR",
                self.root.join("containerd-rootlesskit"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .process_group(0)
            .spawn()
            .expect("spawn isolated dockerd-rootless.sh");
        let process_group = child.id() as i32;
        let process = RootlessProcess {
            child,
            process_group,
        };
        self.process = Some(process);

        let endpoint = self.endpoint();
        let child = &mut self.process.as_mut().expect("daemon process exists").child;
        let ready = wait_for_daemon(endpoint, child).await;
        if let Err(error) = ready {
            panic!(
                "isolated rootless daemon did not become ready: {error}; log: {}",
                self.root.join("dockerd.log").display()
            );
        }
    }

    pub(super) async fn restart(&mut self) {
        assert!(
            self.process.take().expect("daemon process exists").stop(),
            "the isolated rootless daemon process group did not terminate"
        );
        let _ = fs::remove_file(&self.socket);
        let _ = fs::remove_dir_all(self.root.join("rootlesskit"));
        let _ = fs::remove_dir_all(self.runtime.join("docker"));
        self.launch().await;
    }
}

impl Drop for IsolatedRootlessDaemon {
    fn drop(&mut self) {
        if let Some(process) = self.process.take() {
            process.stop();
        }
        let _ = fs::remove_file(&self.socket);
        let _ = fs::remove_dir_all(&self.runtime);
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct RootlessProcess {
    child: Child,
    process_group: i32,
}

impl RootlessProcess {
    fn stop(mut self) -> bool {
        unsafe {
            libc::kill(-self.process_group, libc::SIGTERM);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            if child_exited_without_reaping(self.child.id()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        // Keep the leader unreaped until after signaling its process group. Its PID
        // remains reserved, so the PGID cannot be recycled between observing exit
        // and sending SIGKILL to any surviving daemon children.
        unsafe {
            libc::kill(-self.process_group, libc::SIGKILL);
        }
        let _ = self.child.wait();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if !process_group_exists(self.process_group) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        !process_group_exists(self.process_group)
    }
}

fn child_exited_without_reaping(pid: u32) -> bool {
    let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
    let result = unsafe {
        libc::waitid(
            libc::P_PID,
            pid,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    result == 0 && unsafe { info.si_pid() } != 0
}

fn process_group_exists(process_group: i32) -> bool {
    let status = unsafe { libc::kill(-process_group, 0) };
    status == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

async fn wait_for_daemon(
    endpoint: crate::execution::docker::engine::DockerEndpoint,
    child: &mut Child,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(90);
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("could not inspect isolated daemon process: {error}"))?
        {
            return Err(format!("isolated daemon process exited with {status}"));
        }
        match endpoint.connect() {
            Ok(docker) => match docker.ping().await {
                Ok(_) => return Ok(()),
                Err(error) if tokio::time::Instant::now() < deadline => {
                    let _ = error;
                }
                Err(error) => return Err(error.to_string()),
            },
            Err(error) if tokio::time::Instant::now() < deadline => {
                let _ = error;
            }
            Err(error) => return Err(error.to_string()),
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

#[cfg(test)]
#[path = "harness/tests.rs"]
mod tests;

pub(super) struct IsolatedContainerCleanup {
    endpoint: crate::execution::docker::engine::DockerEndpoint,
    name: String,
}

impl IsolatedContainerCleanup {
    pub(super) fn new(
        endpoint: crate::execution::docker::engine::DockerEndpoint,
        name: String,
    ) -> Self {
        Self { endpoint, name }
    }
}

impl Drop for IsolatedContainerCleanup {
    fn drop(&mut self) {
        if let crate::execution::docker::engine::DockerEndpoint::Unix { path, .. } = &self.endpoint
        {
            let _ = Command::new("docker")
                .arg("--host")
                .arg(format!("unix://{}", path.display()))
                .args(["rm", "--force", &self.name])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}
