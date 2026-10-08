//! Lockfile-driven Cargo cache preparation through a registry-restricted proxy.
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
    thread,
    time::{Duration, Instant},
};

use super::{ResourceResponse, proxy::HttpsRegistryProxy};

mod cache;
mod lockfile;

pub(super) fn package_identities_from_contents(
    contents: &str,
) -> anyhow::Result<Vec<crate::harness::DependencyPackageIdentity>> {
    lockfile::package_identities_from_contents(contents)
}

const CARGO_FETCH_TIMEOUT: Duration = Duration::from_secs(7 * 60);
const CARGO_REGISTRY_HOSTS: &[&str] = &["index.crates.io", "static.crates.io"];

pub(super) struct Adapter;

impl super::adapter::DependencyAdapter for Adapter {
    fn ecosystem(&self) -> crate::harness::PackageEcosystem {
        crate::harness::PackageEcosystem::Cargo
    }

    fn ecosystem_label(&self) -> &'static str {
        "Cargo"
    }

    fn prepare(
        &self,
        context: &super::adapter::PreparationContext<'_>,
        need: &crate::harness::DependencyNeed,
    ) -> anyhow::Result<ResourceResponse> {
        anyhow::ensure!(
            crate::harness::dependency_decision_allowed(need, context.decision),
            "Cargo dependency request did not pass its exact authorization decision"
        );
        anyhow::ensure!(
            need.kind == crate::harness::DependencyKind::ExistingRestore,
            "Cargo currently supports checksum-backed existing restores only"
        );
        crate::harness::resource_bridge::dependency::validate_lockfile_identity(
            context.worktree,
            need,
        )?;
        self::prepare(
            context.worktree,
            context.cargo_cache,
            context.downloaded_bytes,
        )
    }
}

pub(super) fn persistent_cache_at(state_root: &Path) -> anyhow::Result<PathBuf> {
    cache::persistent_cache_at(state_root)
}

pub(super) fn prepare(
    worktree: &Path,
    cargo_cache: &Path,
    downloaded_bytes: &AtomicUsize,
) -> anyhow::Result<ResourceResponse> {
    let packages = lockfile::collect(worktree)?;
    if packages.is_empty() {
        return Ok(ResourceResponse::prepared(
            "Cargo.lock contains no crates.io packages to retrieve".into(),
        ));
    }
    anyhow::ensure!(
        !worktree.join(".cargo/config").exists() && !worktree.join(".cargo/config.toml").exists(),
        "Cargo project configuration can replace the approved registry source"
    );
    let cargo = locate_cargo(worktree)?;
    let used = downloaded_bytes.load(Ordering::Relaxed);
    anyhow::ensure!(
        used < crate::harness::resource_bridge::MAX_SESSION_BYTES,
        "Cargo dependency download budget is exhausted"
    );
    let remaining = crate::harness::resource_bridge::MAX_SESSION_BYTES - used;
    let proxy = HttpsRegistryProxy::start(
        CARGO_REGISTRY_HOSTS.iter().map(|host| (*host).to_owned()),
        remaining,
    )?;
    let result = run_fetch(&cargo, worktree, cargo_cache, &proxy.url());
    downloaded_bytes.fetch_add(proxy.bytes_received(), Ordering::Relaxed);
    result?;
    cache::verify_locked_packages(cargo_cache, &packages)?;
    Ok(ResourceResponse::prepared(format!(
        "Cargo prepared {} crates.io packages from Cargo.lock; registry checksums were verified and the sandbox remains offline.",
        packages.len()
    )))
}

fn run_fetch(cargo: &Path, worktree: &Path, cache: &Path, proxy: &str) -> anyhow::Result<()> {
    let path = env::var_os("PATH").unwrap_or_default();
    let rustup_home = env::var_os("RUSTUP_HOME").or_else(|| {
        env::var_os("HOME").map(|home| PathBuf::from(home).join(".rustup").into_os_string())
    });
    let mut command = Command::new(cargo);
    command
        .current_dir("/")
        .env_clear()
        .env("PATH", path)
        .env("HOME", cache)
        .env("CARGO_HOME", cache)
        .env("CARGO_NET_OFFLINE", "false")
        .env("CARGO_NET_GIT_FETCH_WITH_CLI", "false")
        .env("CARGO_REGISTRIES_CRATES_IO_PROTOCOL", "sparse")
        .env("CARGO_HTTP_PROXY", proxy)
        .env("CARGO_HTTP_MULTIPLEXING", "false")
        .env("HTTPS_PROXY", proxy)
        .env("HTTP_PROXY", proxy)
        .env("https_proxy", proxy)
        .env("http_proxy", proxy)
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .env("RUSTUP_TOOLCHAIN", "1.98.1")
        .arg("--locked")
        .arg("fetch")
        .arg("--manifest-path")
        .arg(worktree.join("Cargo.toml"))
        .args(["--config", "net.git-fetch-with-cli=false"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(rustup_home) = rustup_home {
        command.env("RUSTUP_HOME", rustup_home);
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + CARGO_FETCH_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::ensure!(
                status.success(),
                "Host Cargo could not fetch the verified crates.io lockfile (exit {status})"
            );
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Cargo dependency preparation exceeded seven minutes");
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn locate_cargo(worktree: &Path) -> anyhow::Result<PathBuf> {
    let path = env::var_os("PATH")
        .ok_or_else(|| anyhow::anyhow!("Host PATH is unavailable for Cargo preparation"))?;
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    locate_cargo_in_path(worktree, &path, home.as_deref())
}

fn locate_cargo_in_path(
    worktree: &Path,
    path: &std::ffi::OsStr,
    home: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let worktree = worktree.canonicalize()?;
    for directory in env::split_paths(path).filter(|directory| directory.is_absolute()) {
        let Ok(directory) = directory.canonicalize() else {
            continue;
        };
        let candidate = directory.join("cargo");
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        let metadata = canonical.metadata()?;
        let trusted_directory = ["/usr", "/bin", "/usr/local"]
            .iter()
            .any(|root| directory.starts_with(root))
            || home.is_some_and(|home| directory.starts_with(home.join(".cargo/bin")));
        let trusted_target = ["/usr", "/bin", "/usr/local"]
            .iter()
            .any(|root| canonical.starts_with(root))
            || home.is_some_and(|home| canonical.starts_with(home.join(".cargo/bin")));
        if !metadata.is_file()
            || metadata.permissions().mode() & 0o111 == 0
            || canonical.starts_with(&worktree)
            || !trusted_directory
            || !trusted_target
        {
            continue;
        }
        // Keep the verified `cargo` alias path. In particular, ~/.cargo/bin/cargo
        // is commonly a symlink to `rustup`; invoking the canonical rustup path
        // loses the Cargo proxy name and interprets Cargo flags as rustup flags.
        return Ok(candidate);
    }
    anyhow::bail!("Host Cargo is unavailable; install Rust 1.98.1 and resume the task")
}

#[cfg(unix)]
pub(super) fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Cargo cache path contains a symlink or non-directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(path)?,
        Err(error) => return Err(error.into()),
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(test)]
mod tests;
