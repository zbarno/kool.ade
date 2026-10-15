//! Bounded subprocess helper for optional CLI discovery operations.

use std::{
    cell::RefCell,
    process::{Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

mod process;
mod result;
use process::{is_cancelled, spawn_reader, terminate_group, wait_bounded};

pub(crate) use result::{
    ExecutionCapability, ModelCatalogStatus, ProbeDiagnosticCode, ProbeKind, ProbeResult,
    ProbeStatus,
};

pub(crate) const MODEL_PROBE_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const MAX_PROBE_OUTPUT_BYTES: usize = 1024 * 1024;
const MAX_READER_SHUTDOWN: Duration = Duration::from_millis(500);

/// Retain only lookup, user configuration, locale, and platform runtime paths
/// for a provider probe. Provider credentials and arbitrary shell hooks stay
/// out of every discovery child.
pub(crate) fn sanitize_probe_environment(command: &mut Command) {
    command.env_clear();
    for name in [
        "PATH",
        "PATHEXT",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "LANG",
        "LC_ALL",
        "SYSTEMROOT",
        "WINDIR",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

thread_local! {
    static PROBE_CANCELLATION: RefCell<Option<Arc<std::sync::atomic::AtomicBool>>> = const { RefCell::new(None) };
}

/// Runs one provider discovery operation with a cancellation token inherited by its probes.
pub(crate) fn with_cancellation<T>(
    cancellation: Arc<std::sync::atomic::AtomicBool>,
    operation: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<Arc<std::sync::atomic::AtomicBool>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            let previous = self.0.take();
            PROBE_CANCELLATION.with(|slot| *slot.borrow_mut() = previous);
        }
    }

    let previous = PROBE_CANCELLATION.with(|slot| slot.borrow_mut().replace(cancellation));
    let _restore = Restore(previous);
    operation()
}

pub(crate) fn cancellation_requested() -> bool {
    PROBE_CANCELLATION.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|token| token.load(std::sync::atomic::Ordering::Acquire))
    })
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProbeOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub success: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

pub(crate) fn run(
    argv: &[String],
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    excluded_environment: &[&str],
) -> anyhow::Result<ProbeOutput> {
    run_inner(
        argv,
        timeout,
        stdout_limit,
        stderr_limit,
        excluded_environment,
        &[],
        false,
    )
}

/// Runs a provider capability probe with a task-local, empty HOME and XDG
/// directories. This prevents `--help` startup hooks and plugin discovery
/// from consulting the operator's profile.
pub(crate) fn run_isolated(
    argv: &[String],
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    excluded_environment: &[&str],
) -> anyhow::Result<ProbeOutput> {
    run_inner(
        argv,
        timeout,
        stdout_limit,
        stderr_limit,
        excluded_environment,
        &[],
        true,
    )
}

fn run_inner(
    argv: &[String],
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
    excluded_environment: &[&str],
    environment_overrides: &[(&str, &str)],
    isolate_home: bool,
) -> anyhow::Result<ProbeOutput> {
    anyhow::ensure!(
        !argv.is_empty() && !argv[0].is_empty(),
        "probe command is empty"
    );
    anyhow::ensure!(timeout > Duration::ZERO, "probe timeout must be positive");
    anyhow::ensure!(
        stdout_limit > 0 && stderr_limit > 0,
        "probe output limits must be positive"
    );
    let directory =
        crate::harness::execution_security::PrivateDirectory::create("koolade-cli-probe")?;
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .current_dir(&directory.path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Provider discovery needs a small set of user config and executable
    // lookup variables. Do not inherit arbitrary tokens, cloud credentials,
    // shell startup configuration, or project-specific process settings.
    sanitize_probe_environment(&mut command);
    if isolate_home {
        for child in ["config", "data", "cache", "appdata", "localdata"] {
            std::fs::create_dir_all(directory.path.join(child))?;
        }
        let home = directory.path.to_string_lossy();
        command
            .env("HOME", home.as_ref())
            .env("USERPROFILE", home.as_ref())
            .env("XDG_CONFIG_HOME", directory.path.join("config"))
            .env("XDG_DATA_HOME", directory.path.join("data"))
            .env("XDG_CACHE_HOME", directory.path.join("cache"))
            .env("APPDATA", directory.path.join("appdata"))
            .env("LOCALAPPDATA", directory.path.join("localdata"))
            .env("NO_COLOR", "1")
            .env("TERM", "dumb");
    }
    // Applying overrides before the denylist lets tests prove that even a
    // deliberately injected hostile parent value cannot reach a probe.
    command.envs(environment_overrides.iter().copied());
    for &name in crate::harness::execution_security::UNSAFE_PROVIDER_LAUNCH_ENV {
        command.env_remove(name);
    }
    for name in excluded_environment {
        command.env_remove(name);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    #[cfg(windows)]
    let process_tree = match windows::WindowsProcessTree::attach(pid) {
        Ok(tree) => Some(tree),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("could not supervise provider probe process tree: {error}");
        }
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("probe stdout pipe is unavailable"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("probe stderr pipe is unavailable"))?;
    let stdout_capture = Arc::new(Mutex::new(Capture::new(stdout_limit)));
    let stderr_capture = Arc::new(Mutex::new(Capture::new(stderr_limit)));
    let (finished_tx, finished_rx) = mpsc::channel();
    spawn_reader(stdout, stdout_capture.clone(), finished_tx.clone());
    spawn_reader(stderr, stderr_capture.clone(), finished_tx.clone());

    let deadline = Instant::now() + timeout;
    let cancellation = PROBE_CANCELLATION.with(|slot| slot.borrow().clone());
    let status = wait_bounded(&mut child, pid, deadline, cancellation.as_deref())?;
    #[cfg(windows)]
    drop(process_tree);
    let mut readers_finished = 0;
    while readers_finished < 2 && Instant::now() < deadline + MAX_READER_SHUTDOWN {
        if is_cancelled(cancellation.as_deref()) {
            terminate_group(pid);
            anyhow::bail!("probe was cancelled");
        }
        match finished_rx.recv_timeout(Duration::from_millis(20)) {
            Ok(()) => readers_finished += 1,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    anyhow::ensure!(
        readers_finished == 2,
        "probe descendants kept output pipes open"
    );
    let stdout = stdout_capture
        .lock()
        .map_err(|_| anyhow::anyhow!("probe output capture was poisoned"))?
        .clone();
    let stderr = stderr_capture
        .lock()
        .map_err(|_| anyhow::anyhow!("probe output capture was poisoned"))?
        .clone();
    Ok(ProbeOutput {
        stdout: stdout.bytes,
        stderr: stderr.bytes,
        success: status.success(),
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
    })
}

#[derive(Clone, Debug)]
pub(super) struct Capture {
    bytes: Vec<u8>,
    limit: usize,
    truncated: bool,
}

impl Capture {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit.min(64 * 1024)),
            limit,
            truncated: false,
        }
    }
    pub(super) fn append(&mut self, chunk: &[u8]) {
        let remaining = self.limit.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        self.truncated |= chunk.len() > remaining;
    }
}

#[cfg(windows)]
pub(crate) mod windows;

#[cfg(all(test, unix))]
#[path = "discovery/tests.rs"]
mod tests;
