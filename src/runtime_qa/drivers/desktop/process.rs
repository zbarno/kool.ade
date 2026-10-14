use std::{
    io::{BufRead, BufReader},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
};

use super::display::DISPLAY_NAME;
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
        let mut child = command
            .spawn()
            .map_err(|error| format!("could not launch approved QA target: {error}"))?;
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

    pub(super) fn logs(&self) -> Vec<String> {
        self.logs
            .lock()
            .map(|logs| logs.clone())
            .unwrap_or_default()
    }
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
