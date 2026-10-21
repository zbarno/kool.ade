use std::{
    fs,
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
};

use sha2::Digest;

const CHROMIUM_VERSION: &str = "143.0.7499.4";

pub(super) fn chromium_executable() -> Result<(std::path::PathBuf, String), String> {
    let root = fs::canonicalize("/ms-playwright").map_err(io_error)?;
    let mut matches = Vec::new();
    for entry in fs::read_dir(&root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if !entry.file_name().to_string_lossy().starts_with("chromium-") {
            continue;
        }
        let candidate = entry.path().join("chrome-linux64/chrome");
        if candidate.is_file() && candidate.starts_with(&root) {
            matches.push(candidate);
        }
    }
    if matches.len() != 1 {
        return Err("pinned QA image must contain exactly one packaged Chromium executable".into());
    }
    let executable = fs::canonicalize(&matches[0]).map_err(io_error)?;
    let output = Command::new(&executable)
        .arg("--version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .map_err(io_error)?;
    let version_output = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let version = version_output
        .strip_prefix("Google Chrome for Testing ")
        .unwrap_or("")
        .to_owned();
    if !output.status.success() || version != CHROMIUM_VERSION {
        return Err(format!(
            "packaged Chromium version is not pinned: {version_output}"
        ));
    }
    Ok((executable, version))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

pub(super) fn io_error(error: std::io::Error) -> String {
    format!("packaged Chromium fixture failed: {error}")
}

pub(super) struct ChromiumProcess(pub(super) Child);

impl ChromiumProcess {
    pub(super) fn start(
        executable: &std::path::Path,
        root: &std::path::Path,
    ) -> Result<Self, String> {
        let session_bus = std::path::Path::new("/usr/bin/dbus-run-session");
        let metadata = fs::symlink_metadata(session_bus).map_err(io_error)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err("pinned QA image does not provide a safe private D-Bus launcher".into());
        }
        let mut command = Command::new(session_bus);
        command.arg("--").arg(executable);
        command
            .arg(format!(
                "--user-data-dir={}",
                root.join("profile").display()
            ))
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-sync",
                "--disable-extensions",
                "--disable-component-update",
                "--disable-gpu",
                "--disable-dev-shm-usage",
                "--remote-debugging-address=127.0.0.1",
                "--remote-debugging-port=0",
                // The pinned image omits chrome-sandbox; the fixture runs inside
                // a network-isolated, capability-dropped QA container.
                "--no-sandbox",
            ])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", root)
            .env("DISPLAY", ":99")
            .env("LANG", "C.UTF-8")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(
                fs::File::create(root.join("chromium.stderr")).map_err(io_error)?,
            ));
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        command.spawn().map(Self).map_err(io_error)
    }

    pub(super) fn stop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            unsafe { libc::kill(-(self.0.id() as i32), libc::SIGKILL) };
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for ChromiumProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(super) struct FixtureDirectory(pub(super) std::path::PathBuf);

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
