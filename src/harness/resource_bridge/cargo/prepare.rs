use super::super::{ResourceResponse, proxy::HttpsRegistryProxy};
use super::{cache, lockfile};
use std::{
    env,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicUsize,
    thread,
    time::{Duration, Instant},
};

const CARGO_FETCH_TIMEOUT: Duration = Duration::from_secs(7 * 60);
const CARGO_REGISTRY_HOSTS: &[&str] = &["index.crates.io", "static.crates.io"];

pub(super) fn prepare_with_download_policy(
    worktree: &Path,
    cargo_cache: &Path,
    downloaded_bytes: &AtomicUsize,
    allow_downloads: bool,
) -> anyhow::Result<ResourceResponse> {
    let packages = lockfile::collect(worktree)?;
    if packages.is_empty() {
        return Ok(ResourceResponse::prepared(
            "Cargo.lock contains no crates.io packages to retrieve".into(),
        )
        .with_preparation(crate::harness::DependencyPreparationTelemetry {
            status: Some(crate::harness::DependencyPreparationStatus::AlreadyAvailable),
            ..Default::default()
        }));
    }
    anyhow::ensure!(
        !worktree.join(".cargo/config").exists() && !worktree.join(".cargo/config.toml").exists(),
        "Cargo project configuration can replace the approved registry source"
    );
    let cache_hits = cache::verified_package_count(cargo_cache, &packages)?;
    if !allow_downloads && cache_hits != packages.len() {
        return Ok(ResourceResponse::needs_attention(
            "Fresh Cargo package downloads are disabled while private project configuration is mounted; one or more verified lockfile packages are missing from the local cache.".into(),
        ));
    }
    if !allow_downloads {
        cache::verify_locked_packages(cargo_cache, &packages)?;
        return Ok(ResourceResponse::prepared(format!(
            "Cargo verified all {} crates.io packages in the existing cache; fresh downloads are disabled while private project configuration is mounted.",
            packages.len()
        ))
        .with_preparation(crate::harness::DependencyPreparationTelemetry {
            status: Some(crate::harness::DependencyPreparationStatus::AlreadyAvailable),
            package_count: packages.len() as u64,
            cache_hits: cache_hits as u64,
            ..Default::default()
        }));
    }
    if cache_hits == packages.len() {
        cache::verify_locked_packages(cargo_cache, &packages)?;
        return Ok(ResourceResponse::prepared(format!(
            "Cargo verified all {} crates.io packages in the existing cache; no downloads were needed.",
            packages.len()
        ))
        .with_preparation(crate::harness::DependencyPreparationTelemetry {
            status: Some(crate::harness::DependencyPreparationStatus::AlreadyAvailable),
            package_count: packages.len() as u64,
            cache_hits: cache_hits as u64,
            ..Default::default()
        }));
    }
    let cargo = locate_cargo(worktree)?;
    let reservation = crate::harness::resource_bridge::budget::reserve_downloads(
        downloaded_bytes,
        crate::harness::resource_bridge::MAX_SESSION_BYTES,
        crate::harness::resource_bridge::MAX_SESSION_BYTES,
    )?;
    let proxy = match HttpsRegistryProxy::start(
        CARGO_REGISTRY_HOSTS.iter().map(|host| (*host).to_owned()),
        reservation,
    ) {
        Ok(proxy) => proxy,
        Err(error) => {
            crate::harness::resource_bridge::budget::settle_downloads(
                downloaded_bytes,
                reservation,
                0,
            );
            return Err(error);
        }
    };
    let result = run_fetch(&cargo, worktree, cargo_cache, &proxy.url());
    let bytes_downloaded = proxy.bytes_received();
    crate::harness::resource_bridge::budget::settle_downloads(
        downloaded_bytes,
        reservation,
        bytes_downloaded,
    );
    result?;
    cache::verify_locked_packages(cargo_cache, &packages)?;
    Ok(ResourceResponse::prepared(format!(
        "Cargo prepared {} crates.io packages from Cargo.lock; registry checksums were verified and the sandbox remains offline.",
        packages.len()
    ))
    .with_preparation(crate::harness::DependencyPreparationTelemetry {
        status: Some(if cache_hits == packages.len() && bytes_downloaded == 0 {
            crate::harness::DependencyPreparationStatus::AlreadyAvailable
        } else {
            crate::harness::DependencyPreparationStatus::Prepared
        }),
        package_count: packages.len() as u64,
        cache_hits: cache_hits as u64,
        packages_downloaded: packages.len().saturating_sub(cache_hits) as u64,
        bytes_downloaded: bytes_downloaded as u64,
        ..Default::default()
    }))
}

pub(super) fn run_fetch(
    cargo: &Path,
    worktree: &Path,
    cache: &Path,
    proxy: &str,
) -> anyhow::Result<()> {
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

pub(super) fn locate_cargo_in_path(
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
