use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

use super::display::DISPLAY_NAME;
use crate::execution::contract::DependencyCacheEcosystemV1;
use crate::runtime_qa::{QaRunRequestV1, QaValidatedTargetV1};

const LOG_LIMIT: usize = 256 * 1024;

pub(super) struct TargetProcess {
    child: Child,
    logs: Arc<Mutex<Vec<String>>>,
}

impl TargetProcess {
    pub(super) fn launch(
        request: &QaRunRequestV1,
        target: &QaValidatedTargetV1,
    ) -> Result<Self, String> {
        target.validate_for(request)?;
        let workspace = std::fs::canonicalize("/workspace")
            .map_err(|error| format!("QA workspace is unavailable: {error}"))?;
        let executable = std::fs::canonicalize(&target.executable)
            .map_err(|error| format!("approved executable is unavailable: {error}"))?;
        let working_directory = std::fs::canonicalize(&target.working_directory)
            .map_err(|error| format!("approved working directory is unavailable: {error}"))?;
        let executable_metadata = std::fs::symlink_metadata(&executable)
            .map_err(|error| format!("could not inspect approved executable: {error}"))?;
        if !executable.starts_with(&workspace)
            || !working_directory.starts_with(&workspace)
            || !executable_metadata.is_file()
            || executable_metadata.permissions().mode() & 0o111 == 0
        {
            return Err(
                "approved target resolves outside the immutable workspace or is not executable"
                    .into(),
            );
        }
        let mut command = Command::new(executable);
        command
            .args(&target.arguments)
            .current_dir(working_directory)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/tmp")
            .env("DISPLAY", DISPLAY_NAME)
            .env("XDG_RUNTIME_DIR", "/tmp")
            .env("LANG", "C.UTF-8")
            .env("GDK_BACKEND", "x11")
            .env("QT_QPA_PLATFORM", "xcb")
            .env("LIBGL_ALWAYS_SOFTWARE", "1");
        let ecosystems = request
            .dependency_cache_refs
            .iter()
            .map(|dependency| dependency.cache_ref.ecosystem)
            .collect::<BTreeSet<_>>();
        if ecosystems.contains(&DependencyCacheEcosystemV1::Npm) {
            command
                .env("NPM_CONFIG_CACHE", "/home/executor/.npm")
                .env("NPM_CONFIG_OFFLINE", "true");
        }
        if ecosystems.contains(&DependencyCacheEcosystemV1::Cargo) {
            command
                .env("CARGO_HOME", "/home/executor/.cargo")
                .env("CARGO_NET_OFFLINE", "true");
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // The driver process runs in the disposable, network-isolated QA container.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().map_err(|error| {
            format!(
                "could not launch approved QA target: {error} ({})",
                launch_capability_diagnostic(&error)
            )
        })?;
        let logs = Arc::new(Mutex::new(Vec::new()));
        drain(child.stdout.take(), Arc::clone(&logs));
        drain(child.stderr.take(), Arc::clone(&logs));
        Ok(Self { child, logs })
    }

    pub(super) fn is_running(&mut self) -> Result<bool, String> {
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|error| format!("could not inspect QA target process: {error}"))
    }

    pub(super) fn owned_pids(&self) -> BTreeSet<u32> {
        descendants(self.child.id())
    }

    pub(super) fn logs(&self) -> Vec<String> {
        self.logs
            .lock()
            .map(|logs| logs.clone())
            .unwrap_or_default()
    }
}

pub(super) fn descendants(root: u32) -> BTreeSet<u32> {
    let mut result = BTreeSet::from([root]);
    let mut pending = vec![root];
    while let Some(pid) = pending.pop() {
        if result.len() >= 4096 {
            break;
        }
        let tasks = format!("/proc/{pid}/task");
        let Ok(tasks) = std::fs::read_dir(tasks) else {
            continue;
        };
        for task in tasks.flatten() {
            let path = task.path().join("children");
            let Ok(children) = std::fs::read_to_string(path) else {
                continue;
            };
            for child in children
                .split_whitespace()
                .filter_map(|value| value.parse::<u32>().ok())
            {
                if result.insert(child) {
                    pending.push(child);
                }
            }
        }
    }
    result
}

impl Drop for TargetProcess {
    fn drop(&mut self) {
        // setsid makes the child PID its process group; close descendants as well.
        if self.child.try_wait().ok().flatten().is_none() {
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Maps a spawn failure for an approved artifact to the target-capability
/// vocabulary used in QA summaries. The executable path is canonicalized,
/// stat-checked, and exec-bit verified above, so a spawn-time `NotFound` means
/// the dynamic loader could not resolve a shared-object dependency of the
/// packaged artifact rather than the artifact file being absent.
pub(super) fn launch_capability_diagnostic(error: &std::io::Error) -> &'static str {
    if error.kind() == std::io::ErrorKind::NotFound {
        "dynamic loader could not resolve shared-object dependencies of the approved artifact (target capability: missing_dependencies)"
    } else {
        "approved artifact process could not be started (target capability: start_failed)"
    }
}

fn drain(stream: Option<impl std::io::Read + Send + 'static>, logs: Arc<Mutex<Vec<String>>>) {
    if let Some(stream) = stream {
        thread::spawn(move || {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let Ok(mut logs) = logs.lock() else { break };
                let bytes = logs.iter().map(String::len).sum::<usize>();
                if bytes + line.len() <= LOG_LIMIT {
                    logs.push(line.chars().take(4096).collect());
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::launch_capability_diagnostic;

    /// Spawn NotFound on a canonicalized, stat-verified path is a loader
    /// failure (unresolved NEEDED entry), which QA reports as a missing
    /// dependency rather than a pass or a generic start failure.
    #[test]
    fn spawn_not_found_reports_missing_dependencies() {
        let error = std::io::Error::from(std::io::ErrorKind::NotFound);
        let diagnostic = launch_capability_diagnostic(&error);
        assert!(diagnostic.ends_with("(target capability: missing_dependencies)"));
    }

    #[test]
    fn other_spawn_failures_report_start_failed() {
        let error = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(
            launch_capability_diagnostic(&error).ends_with("(target capability: start_failed)")
        );
    }
}
