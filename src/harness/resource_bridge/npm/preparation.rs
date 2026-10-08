use super::{ResourceResponse, cache, lockfile};
use crate::harness::resource_bridge::{fetch, policy};
use anyhow::Context;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

const CACHE_ADD_BATCH: usize = 100;
const CACHE_ADD_TIMEOUT: Duration = Duration::from_secs(300);
const PREPARE_TIMEOUT: Duration = Duration::from_secs(7 * 60);

pub(super) type NpmArchiveRetriever<'a> = dyn Fn(&Path, &str, &str, u64, Option<&url::Url>) -> anyhow::Result<ResourceResponse>
    + Send
    + Sync
    + 'a;
pub(super) type NpmCacheIndexer<'a> =
    dyn Fn(&Path, &Path, &[PathBuf], Duration) -> anyhow::Result<()> + Send + Sync + 'a;
pub(super) type NpmAdditionResolver<'a> =
    dyn Fn(&Path, &Path, &str, &str) -> anyhow::Result<()> + Send + Sync + 'a;

pub(in crate::harness::resource_bridge) struct SharedPreparationOperations {
    pub(super) retrieve: std::sync::Arc<NpmArchiveRetriever<'static>>,
    pub(super) index_cache: std::sync::Arc<NpmCacheIndexer<'static>>,
    pub(super) resolve_addition: std::sync::Arc<NpmAdditionResolver<'static>>,
}

pub(super) struct PreparationOperations<'a> {
    pub(super) retrieve: &'a NpmArchiveRetriever<'a>,
    pub(super) index_cache: &'a NpmCacheIndexer<'a>,
}

pub(super) struct PreparationRequest<'a> {
    pub(super) worktree: &'a Path,
    pub(super) response_dir: &'a Path,
    pub(super) npm_cache: &'a Path,
    pub(super) npm_snapshot: &'a Path,
    pub(super) purpose: &'a str,
    pub(super) downloaded_bytes: &'a AtomicUsize,
    pub(super) authorized_registry: Option<&'a url::Url>,
}

pub(super) fn prepare_with_registry(
    worktree: &Path,
    response_dir: &Path,
    npm_cache: &Path,
    npm_snapshot: &Path,
    purpose: &str,
    downloaded_bytes: &AtomicUsize,
    authorized_registry: Option<&url::Url>,
) -> anyhow::Result<ResourceResponse> {
    prepare_with_registry_and_ops(
        PreparationRequest {
            worktree,
            response_dir,
            npm_cache,
            npm_snapshot,
            purpose,
            downloaded_bytes,
            authorized_registry,
        },
        PreparationOperations {
            retrieve: &|response_dir, url, purpose, remaining, registry| match registry {
                Some(registry) => {
                    fetch::retrieve_npm_registry(response_dir, url, purpose, remaining, registry)
                }
                None => fetch::retrieve(response_dir, url, purpose, remaining),
            },
            index_cache: &|worktree, npm_cache, archives, timeout| {
                let npm = cache::locate_npm(worktree)?;
                cache::add_to_cache(&npm, npm_cache, archives, timeout)
            },
        },
    )
}

pub(super) fn prepare_with_registry_and_ops(
    request: PreparationRequest<'_>,
    operations: PreparationOperations<'_>,
) -> anyhow::Result<ResourceResponse> {
    let PreparationRequest {
        worktree,
        response_dir,
        npm_cache,
        npm_snapshot,
        purpose,
        downloaded_bytes,
        authorized_registry,
    } = request;
    let deadline = Instant::now() + PREPARE_TIMEOUT;
    anyhow::ensure!(
        purpose.trim().len() >= 3 && !purpose.chars().any(char::is_control),
        "Explain why the npm lockfile dependencies are needed"
    );
    let packages = match lockfile::collect_lockfiles(worktree)
        .context("scanning npm lockfiles for verified package archives")
    {
        Ok(packages) => packages,
        Err(error) => {
            return Ok(ResourceResponse::needs_attention(format!(
                "Kool.ad/e could not prepare npm dependencies automatically: {error:#}"
            )));
        }
    };
    if packages.is_empty() {
        return Ok(ResourceResponse::prepared(
            "npm lockfiles contain no remote package archives to retrieve".into(),
        )
        .with_preparation(crate::harness::DependencyPreparationTelemetry {
            status: Some(crate::harness::DependencyPreparationStatus::AlreadyAvailable),
            ..Default::default()
        }));
    }
    anyhow::ensure!(
        packages.len() <= super::MAX_LOCKED_PACKAGES,
        "npm lockfiles contain more than {} remote package archives",
        super::MAX_LOCKED_PACKAGES
    );

    let mut missing = Vec::new();
    let mut cached = 0_usize;
    let mut transferred = 0_usize;
    for package in &packages {
        anyhow::ensure!(
            Instant::now() < deadline,
            "npm dependency preparation exceeded seven minutes"
        );
        let decision = match authorized_registry {
            Some(registry) => policy::classify_npm_registry_package(&package.url, registry),
            None => policy::classify(&package.url),
        };
        match decision {
            Ok(policy::Decision::Allow(_)) => {}
            Ok(policy::Decision::NeedsAttention(detail)) => {
                return Ok(ResourceResponse::needs_attention(detail));
            }
            Err(error) => {
                return Ok(ResourceResponse::needs_attention(format!(
                    "The npm lockfile contains a dependency URL that needs operator review: {error:#}"
                )));
            }
        }
        let Some(digest_path) = lockfile::npm_cache_digest_path(npm_cache, &package.integrity)
        else {
            return Ok(ResourceResponse::needs_attention(
                "An npm lockfile entry has no supported SHA-512 integrity value. Kool.ad/e will not fetch it without a verifiable lockfile digest.".into(),
            ));
        };
        if lockfile::verify_sha512_file(&digest_path, &package.integrity) {
            cached += 1;
            continue;
        }
        let already = downloaded_bytes.load(Ordering::Relaxed);
        anyhow::ensure!(
            already < super::super::MAX_SESSION_BYTES,
            "npm dependency download budget is exhausted"
        );
        let reservation =
            (super::super::MAX_SESSION_BYTES - already).min(fetch::MAX_RESOURCE_BYTES as usize);
        downloaded_bytes.fetch_add(reservation, Ordering::Relaxed);
        let response = (operations.retrieve)(
            response_dir,
            &package.url,
            purpose,
            reservation as u64,
            authorized_registry,
        );
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                let received = crate::harness::resource_bridge::fetch::bytes_from_failure(&error)
                    .unwrap_or_default();
                downloaded_bytes.fetch_sub(reservation.saturating_sub(received), Ordering::Relaxed);
                return Err(error).context("retrieving a lockfile-pinned npm package");
            }
        };
        downloaded_bytes.fetch_sub(
            reservation.saturating_sub(response.bytes),
            Ordering::Relaxed,
        );
        if response.status != "allowed" {
            return Ok(response);
        }
        let virtual_path = response.path.as_deref().ok_or_else(|| {
            anyhow::anyhow!("npm registry response did not produce a package archive")
        })?;
        let name = Path::new(virtual_path)
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("npm resource path has no filename"))?;
        let source = response_dir
            .join(name)
            .canonicalize()
            .context("locating the verified npm response archive")?;
        let response_root = response_dir
            .canonicalize()
            .context("locating the private npm response directory")?;
        anyhow::ensure!(
            source.starts_with(response_root) && source.is_file(),
            "npm package archive escaped the private resource cache"
        );
        anyhow::ensure!(
            lockfile::verify_sha512_file(&source, &package.integrity),
            "Downloaded npm archive did not match its lockfile SHA-512 integrity value"
        );
        let target = response_dir.join(format!("{}.tgz", uuid::Uuid::new_v4()));
        fs::copy(&source, &target)
            .context("staging the verified npm archive for cache indexing")?;
        transferred = transferred.saturating_add(response.bytes);
        missing.push(target);
    }
    if !missing.is_empty() {
        for batch in missing.chunks(CACHE_ADD_BATCH) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            anyhow::ensure!(
                !remaining.is_zero(),
                "npm dependency preparation exceeded seven minutes"
            );
            (operations.index_cache)(worktree, npm_cache, batch, remaining.min(CACHE_ADD_TIMEOUT))
                .context("indexing verified npm archives into the isolated cache")?;
        }
        cache::publish_index_snapshot(npm_cache, npm_snapshot)
            .context("publishing an immutable npm cache index snapshot")?;
    }
    let packages_downloaded = missing.len() as u64;
    Ok(ResourceResponse::prepared(format!(
        "npm cache ready: {cached} packages reused and {packages_downloaded} lockfile-pinned packages retrieved ({transferred} bytes). npm install commands remain offline and run package scripts only inside the sandbox.",
    ))
    .with_preparation(crate::harness::DependencyPreparationTelemetry {
        status: Some(if packages_downloaded == 0 {
            crate::harness::DependencyPreparationStatus::AlreadyAvailable
        } else {
            crate::harness::DependencyPreparationStatus::Prepared
        }),
        package_count: packages.len() as u64,
        cache_hits: cached as u64,
        packages_downloaded,
        bytes_downloaded: transferred as u64,
        ..Default::default()
    }))
}
