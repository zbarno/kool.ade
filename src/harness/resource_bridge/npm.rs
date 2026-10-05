//! Lockfile-driven npm cache preparation through the mediated registry fetcher.
use super::{ResourceResponse, fetch, policy};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

mod cache;
mod lockfile;
#[cfg(test)]
mod tests;

const MAX_LOCKED_PACKAGES: usize = lockfile::MAX_LOCKED_PACKAGES;
const CACHE_ADD_BATCH: usize = 100;
const CACHE_ADD_TIMEOUT: Duration = Duration::from_secs(300);
const PREPARE_TIMEOUT: Duration = Duration::from_secs(7 * 60);

pub(super) fn persistent_cache() -> anyhow::Result<PathBuf> {
    cache::persistent_cache()
}

pub(super) fn cache_covers_lockfile(worktree: &Path, cache: &Path) -> bool {
    lockfile::cache_covers_lockfile(worktree, cache)
}

pub(super) fn verified_offline_cache(worktree: &Path, cache: &Path) -> bool {
    lockfile::collect_lockfiles(worktree).is_ok_and(|packages| {
        packages.iter().all(|package| {
            lockfile::npm_cache_digest_path(cache, &package.integrity)
                .is_some_and(|path| lockfile::verify_sha512_file(&path, &package.integrity))
        })
    })
}

pub(super) fn prepare(
    worktree: &Path,
    response_dir: &Path,
    npm_cache: &Path,
    purpose: &str,
    downloaded_bytes: &AtomicUsize,
) -> anyhow::Result<ResourceResponse> {
    let deadline = Instant::now() + PREPARE_TIMEOUT;
    anyhow::ensure!(
        purpose.trim().len() >= 3 && !purpose.chars().any(char::is_control),
        "Explain why the npm lockfile dependencies are needed"
    );
    let packages = match lockfile::collect_lockfiles(worktree) {
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
        ));
    }
    anyhow::ensure!(
        packages.len() <= MAX_LOCKED_PACKAGES,
        "npm lockfiles contain more than {MAX_LOCKED_PACKAGES} remote package archives"
    );

    let mut missing = Vec::new();
    let mut cached = 0_usize;
    let mut transferred = 0_usize;
    for package in &packages {
        anyhow::ensure!(
            Instant::now() < deadline,
            "npm dependency preparation exceeded seven minutes"
        );
        match policy::classify(&package.url) {
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
            already < super::MAX_SESSION_BYTES,
            "npm dependency download budget is exhausted"
        );
        let reservation =
            (super::MAX_SESSION_BYTES - already).min(fetch::MAX_RESOURCE_BYTES as usize);
        downloaded_bytes.fetch_add(reservation, Ordering::Relaxed);
        let response = fetch::retrieve(response_dir, &package.url, purpose, reservation as u64)?;
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
        let source = response_dir.join(name);
        let source = source.canonicalize()?;
        anyhow::ensure!(
            source.starts_with(response_dir.canonicalize()?) && source.is_file(),
            "npm package archive escaped the private resource cache"
        );
        anyhow::ensure!(
            lockfile::verify_sha512_file(&source, &package.integrity),
            "Downloaded npm archive did not match its lockfile SHA-512 integrity value"
        );
        let target = response_dir.join(format!("{}.tgz", uuid::Uuid::new_v4()));
        fs::copy(&source, &target)?;
        transferred = transferred.saturating_add(response.bytes);
        missing.push(target);
    }
    if !missing.is_empty() {
        let npm = cache::locate_npm(worktree)?;
        for batch in missing.chunks(CACHE_ADD_BATCH) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            anyhow::ensure!(
                !remaining.is_zero(),
                "npm dependency preparation exceeded seven minutes"
            );
            cache::add_to_cache(&npm, npm_cache, batch, remaining.min(CACHE_ADD_TIMEOUT))?;
        }
    }
    Ok(ResourceResponse::prepared(format!(
        "npm cache ready: {cached} packages reused and {} lockfile-pinned packages retrieved ({transferred} bytes). npm install commands remain offline and run package scripts only inside the sandbox.",
        missing.len()
    )))
}
