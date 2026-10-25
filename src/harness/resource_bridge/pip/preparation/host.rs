use super::DownloadOutcome;
use crate::harness::resource_bridge::proxy::HttpsRegistryProxy;
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const PIP_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(7 * 60);
const PYPI_HOSTS: &[&str] = &["pypi.org", "files.pythonhosted.org"];

pub(super) fn fetch_from_pypi(
    requirements: &Path,
    wheelhouse: &Path,
    limit: usize,
) -> DownloadOutcome {
    let proxy =
        match HttpsRegistryProxy::start(PYPI_HOSTS.iter().map(|host| (*host).to_owned()), limit) {
            Ok(proxy) => proxy,
            Err(error) => {
                return DownloadOutcome {
                    bytes_received: 0,
                    result: Err(error),
                };
            }
        };
    let result = run_download(requirements, wheelhouse, &proxy.url());
    DownloadOutcome {
        bytes_received: proxy.bytes_received(),
        result,
    }
}

fn run_download(requirements: &Path, wheelhouse: &Path, proxy: &str) -> anyhow::Result<()> {
    let python = locate_python()?;
    let path = env::var_os("PATH").unwrap_or_default();
    let staging_root = wheelhouse
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Pip wheelhouse has no staging parent"))?;
    let home = staging_root.join("pip-home");
    fs::create_dir(&home)?;
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
    let mut command = Command::new(python);
    command
        .current_dir("/")
        .env_clear()
        .env("PATH", path)
        .env("HOME", &home)
        .env("TMPDIR", staging_root)
        .env("PIP_CONFIG_FILE", "/dev/null")
        .env("PIP_INDEX_URL", "https://pypi.org/simple")
        .env("PIP_NO_INPUT", "1")
        .env("PIP_DISABLE_PIP_VERSION_CHECK", "1")
        .env("PYTHONNOUSERSITE", "1")
        .env("HTTPS_PROXY", proxy)
        .env("HTTP_PROXY", proxy)
        .env("https_proxy", proxy)
        .env("http_proxy", proxy)
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .args(pip_download_arguments())
        .arg(wheelhouse)
        .arg("--requirement")
        .arg(requirements)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    let deadline = Instant::now() + PIP_DOWNLOAD_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::ensure!(
                status.success(),
                "Host pip could not fetch hash-pinned universal PyPI wheels (exit {status})"
            );
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Pip dependency preparation exceeded seven minutes");
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn pip_download_arguments() -> [&'static str; 15] {
    [
        "-I",
        "-m",
        "pip",
        "download",
        "--no-deps",
        "--require-hashes",
        "--only-binary=:all:",
        "--platform",
        "any",
        "--python-version",
        crate::execution::docker::images::PINNED_PYTHON_RUNTIME,
        "--no-cache-dir",
        "--disable-pip-version-check",
        "--no-input",
        "--dest",
    ]
}

fn locate_python() -> anyhow::Result<PathBuf> {
    let path = env::var_os("PATH")
        .ok_or_else(|| anyhow::anyhow!("Host PATH is unavailable for Pip preparation"))?;
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    for directory in env::split_paths(&path).filter(|directory| directory.is_absolute()) {
        let Ok(directory) = directory.canonicalize() else {
            continue;
        };
        let candidate = directory.join("python3");
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        let Ok(metadata) = canonical.metadata() else {
            continue;
        };
        let trusted_directory = ["/usr", "/bin", "/usr/local"]
            .iter()
            .any(|root| directory.starts_with(root))
            || home
                .as_ref()
                .is_some_and(|home| directory.starts_with(home.join(".local/bin")));
        let trusted_target = ["/usr", "/bin", "/usr/local"]
            .iter()
            .any(|root| canonical.starts_with(root))
            || home
                .as_ref()
                .is_some_and(|home| canonical.starts_with(home.join(".local")));
        if metadata.is_file()
            && metadata.permissions().mode() & 0o111 != 0
            && trusted_directory
            && trusted_target
        {
            return Ok(candidate);
        }
    }
    anyhow::bail!("A trusted host Python 3 interpreter is unavailable")
}

#[cfg(test)]
mod tests {
    use super::pip_download_arguments;

    #[test]
    fn host_pip_resolves_against_the_pinned_executor_python_version() {
        let arguments = pip_download_arguments();
        let version = arguments
            .windows(2)
            .find(|pair| pair[0] == "--python-version")
            .map(|pair| pair[1]);
        assert_eq!(version, Some("3.14.7"));
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["--platform", "any"])
        );
    }
}
