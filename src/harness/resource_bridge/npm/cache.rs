//! Isolated persistent npm cache and trusted host npm runner.
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

mod snapshots;
#[cfg(test)]
mod test_support;
pub(super) use snapshots::publish_index_snapshot;
#[cfg(test)]
pub(super) use test_support::{add_test_registry_entry, add_test_registry_packument};

struct EmptyGlobalConfig(PathBuf);

impl EmptyGlobalConfig {
    fn create() -> anyhow::Result<Self> {
        let path =
            env::temp_dir().join(format!("koolade-npm-global-{}.npmrc", uuid::Uuid::new_v4()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(b"")?;
        Ok(Self(path))
    }
}

impl Drop for EmptyGlobalConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(super) fn persistent_cache_at(
    state_root: &Path,
    task_repository: &Path,
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(state_root)?;
    let state_root = state_root.canonicalize()?;
    let task_repository = task_repository.canonicalize()?;
    let task_key = format!(
        "{:x}",
        Sha256::digest(task_repository.to_string_lossy().as_bytes())
    );
    let mut directory = state_root.clone();
    for component in ["package-caches", "npm", "tasks", &task_key] {
        directory.push(component);
        ensure_private_directory(&directory)?;
        anyhow::ensure!(
            directory.canonicalize()?.starts_with(&state_root),
            "npm package cache escapes the Kool.ad/e state directory"
        );
    }
    let content = directory.join("_cacache");
    ensure_private_directory(&content)?;
    ensure_private_directory(&content.join("content-v2"))?;
    ensure_private_directory(&content.join("index-v5"))?;
    Ok(directory)
}

fn ensure_private_directory(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Package cache path contains a symlink or non-directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(path)?,
        Err(error) => return Err(error.into()),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub(super) fn locate_npm(worktree: &Path) -> anyhow::Result<PathBuf> {
    let worktree = worktree.canonicalize()?;
    if let Some(root) = crate::harness::pi_sandbox::host_node_root_for_resource_broker()? {
        let candidate = root.join("lib/node_modules/npm/bin/npm-cli.js");
        let canonical = candidate.canonicalize()?;
        anyhow::ensure!(
            canonical.starts_with(&root) && canonical.is_file(),
            "Host npm CLI escapes its validated Node.js installation"
        );
        return Ok(canonical);
    }
    let path = env::var_os("PATH")
        .ok_or_else(|| anyhow::anyhow!("Host PATH is unavailable for npm cache preparation"))?;
    for directory in env::split_paths(&path).filter(|directory| directory.is_absolute()) {
        let candidate = directory.join("npm");
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        let metadata = canonical.metadata()?;
        anyhow::ensure!(
            metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
            "Host npm launcher is not executable"
        );
        if canonical.starts_with(&worktree)
            || !["/usr", "/bin", "/usr/local"]
                .iter()
                .any(|root| canonical.starts_with(root))
        {
            continue;
        }
        return Ok(canonical);
    }
    anyhow::bail!("Host npm is unavailable; install Node.js with npm and resume the task")
}

pub(super) fn add_to_cache(
    npm: &Path,
    cache: &Path,
    archives: &[PathBuf],
    timeout: Duration,
) -> anyhow::Result<()> {
    let path = env::var_os("PATH").unwrap_or_default();
    let global_config = EmptyGlobalConfig::create()?;
    let mut child = Command::new(npm)
        .current_dir("/")
        .env_clear()
        .env("PATH", path)
        .env("HOME", cache)
        .arg("--userconfig=/dev/null")
        .arg(format!("--globalconfig={}", global_config.0.display()))
        .arg("--cache")
        .arg(cache)
        .args(["cache", "add", "--offline", "--ignore-scripts"])
        .args(archives)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::ensure!(
                status.success(),
                "Host npm could not index downloaded packages into its isolated cache (exit {status})"
            );
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Host npm cache preparation exceeded its time limit");
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

pub(super) fn resolve_lockfile_only(
    npm: &Path,
    cache: &Path,
    project: &Path,
    package_spec: &str,
    registry: &str,
    proxy: &str,
    timeout: Duration,
) -> anyhow::Result<()> {
    let path = env::var_os("PATH").unwrap_or_default();
    let global_config = EmptyGlobalConfig::create()?;
    let mut child = Command::new(npm)
        .current_dir(project)
        .env_clear()
        .env("PATH", path)
        .env("HOME", project)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_ALLOW_PROTOCOL", "https")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("HTTP_PROXY", proxy)
        .env("HTTPS_PROXY", proxy)
        .env("http_proxy", proxy)
        .env("https_proxy", proxy)
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .args(["--userconfig=/dev/null"])
        .arg(format!("--globalconfig={}", global_config.0.display()))
        .args([
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--package-lock-only",
            "--noproxy=",
            "--https-proxy",
            proxy,
            "--proxy",
            proxy,
            "--cache",
        ])
        .arg(cache)
        .arg(format!("--registry={registry}"))
        .arg("install")
        .arg(package_spec)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::ensure!(
                status.success(),
                "Host npm could not resolve the approved public package into an isolated lockfile (exit {status})"
            );
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Host npm package resolution exceeded its time limit");
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}
