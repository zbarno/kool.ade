//! Resolve one approved npm addition and prefill its verified offline cache.
use anyhow::Context;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicUsize,
    time::Duration,
};

use super::{ResourceResponse, cache};
use crate::harness::resource_bridge::proxy::HttpsRegistryProxy;

const RESOLUTION_TIMEOUT: Duration = Duration::from_secs(180);

pub(super) struct AdditionRequest<'a> {
    pub(super) worktree: &'a Path,
    pub(super) response_dir: &'a Path,
    pub(super) npm_cache: &'a Path,
    pub(super) npm_snapshot: &'a Path,
    pub(super) need: &'a crate::harness::DependencyNeed,
    pub(super) decision: crate::harness::DependencyDecision,
    pub(super) downloaded_bytes: &'a AtomicUsize,
}

pub(super) fn prepare(
    worktree: &Path,
    response_dir: &Path,
    npm_cache: &Path,
    npm_snapshot: &Path,
    need: &crate::harness::DependencyNeed,
    decision: crate::harness::DependencyDecision,
    downloaded_bytes: &AtomicUsize,
) -> anyhow::Result<ResourceResponse> {
    prepare_with_resolver_and_operations(
        AdditionRequest {
            worktree,
            response_dir,
            npm_cache,
            npm_snapshot,
            need,
            decision,
            downloaded_bytes,
        },
        |project, npm_cache, package_spec, registry| {
            let npm = cache::locate_npm(worktree)?;
            let used = downloaded_bytes.load(std::sync::atomic::Ordering::Relaxed);
            anyhow::ensure!(
                used < crate::harness::resource_bridge::MAX_SESSION_BYTES,
                "npm dependency download budget is exhausted"
            );
            let remaining = crate::harness::resource_bridge::MAX_SESSION_BYTES - used;
            let registry_host = url::Url::parse(registry)?
                .host_str()
                .ok_or_else(|| anyhow::anyhow!("The requested npm registry has no host"))?
                .to_owned();
            let proxy = HttpsRegistryProxy::start([registry_host], remaining)?;
            let resolved = cache::resolve_lockfile_only(
                &npm,
                npm_cache,
                project,
                package_spec,
                registry,
                &proxy.url(),
                RESOLUTION_TIMEOUT,
            );
            downloaded_bytes
                .fetch_add(proxy.bytes_received(), std::sync::atomic::Ordering::Relaxed);
            resolved
        },
        None,
    )
}

pub(super) fn prepare_with_operations(
    request: AdditionRequest<'_>,
    operations: &super::SharedPreparationOperations,
) -> anyhow::Result<ResourceResponse> {
    prepare_with_resolver_and_operations(
        request,
        |project, cache, package_spec, registry| {
            (operations.resolve_addition)(project, cache, package_spec, registry)
        },
        Some(super::PreparationOperations {
            retrieve: operations.retrieve.as_ref(),
            index_cache: operations.index_cache.as_ref(),
        }),
    )
}

#[cfg(test)]
pub(super) fn prepare_with_resolver(
    request: AdditionRequest<'_>,
    resolve: impl FnOnce(&Path, &str, &str) -> anyhow::Result<()>,
) -> anyhow::Result<ResourceResponse> {
    prepare_with_resolver_and_operations(
        request,
        |project, _, package_spec, registry| resolve(project, package_spec, registry),
        None,
    )
}

fn prepare_with_resolver_and_operations(
    request: AdditionRequest<'_>,
    resolve: impl FnOnce(&Path, &Path, &str, &str) -> anyhow::Result<()>,
    preparation: Option<super::PreparationOperations<'_>>,
) -> anyhow::Result<ResourceResponse> {
    let AdditionRequest {
        worktree,
        response_dir,
        npm_cache,
        npm_snapshot,
        need,
        decision,
        downloaded_bytes,
    } = request;
    anyhow::ensure!(
        need.ecosystem == crate::harness::PackageEcosystem::Npm
            && matches!(
                need.kind,
                crate::harness::DependencyKind::NewProjectDependency
                    | crate::harness::DependencyKind::DevelopmentDependency
            )
            && crate::harness::dependency_decision_allowed(need, decision),
        "npm addition is outside the authorized registry dependency policy"
    );
    crate::harness::resource_bridge::dependency::validate_lockfile_identity(worktree, need)
        .context("validating npm manifests and lockfiles against the authorized request")?;
    let package = need
        .package
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("An exact npm package name is required"))?;
    let version = need
        .version
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("An npm version or range is required"))?;
    let registry =
        crate::harness::resource_bridge::dependency::npm_registry_url(need.source.as_deref())
            .ok_or_else(|| {
                anyhow::anyhow!("The requested npm registry URL is not a safe HTTPS registry root")
            })?;
    let existing_preparation = match prepare_existing_lockfile_with_registry(
        worktree,
        response_dir,
        npm_cache,
        npm_snapshot,
        downloaded_bytes,
        Some(&registry),
    )? {
        Some(existing) if existing.status != "prepared" => return Ok(existing),
        Some(existing) => existing.preparation,
        None => None,
    };
    let project = TemporaryProject::create(response_dir)?;
    fs::write(
        project.path().join("package.json"),
        serde_json::to_vec(&serde_json::json!({
            "name": "koolade-dependency-resolver",
            "version": "1.0.0",
            "private": true,
        }))?,
    )?;
    let package_spec = format!("{package}@{version}");
    resolve(project.path(), npm_cache, &package_spec, registry.as_str())
        .context("resolving the approved npm package into a lockfile")?;
    let mut result = match preparation {
        Some(operations) => super::prepare_for_addition_with_ops(
            super::PreparationRequest {
                worktree: project.path(),
                response_dir,
                npm_cache,
                npm_snapshot,
                purpose: "Prepare artifacts for an authorized npm dependency",
                downloaded_bytes,
                authorized_registry: Some(&registry),
            },
            operations,
        ),
        None => super::prepare_for_addition(
            project.path(),
            response_dir,
            npm_cache,
            npm_snapshot,
            &registry,
            "Prepare artifacts for an authorized npm dependency",
            downloaded_bytes,
        ),
    }
    .context("preparing the lockfile-pinned npm archive in the offline cache")?;
    if let Some(previous) = existing_preparation {
        let current = result
            .preparation
            .get_or_insert_with(crate::harness::DependencyPreparationTelemetry::default);
        current.package_count = current.package_count.saturating_add(previous.package_count);
        current.cache_hits = current.cache_hits.saturating_add(previous.cache_hits);
        current.packages_downloaded = current
            .packages_downloaded
            .saturating_add(previous.packages_downloaded);
        current.bytes_downloaded = current
            .bytes_downloaded
            .saturating_add(previous.bytes_downloaded);
        current.status = Some(if current.packages_downloaded == 0 {
            crate::harness::DependencyPreparationStatus::AlreadyAvailable
        } else {
            crate::harness::DependencyPreparationStatus::Prepared
        });
    }
    Ok(result)
}

#[cfg(test)]
pub(super) fn prepare_existing_lockfile(
    worktree: &Path,
    response_dir: &Path,
    npm_cache: &Path,
    downloaded_bytes: &AtomicUsize,
) -> anyhow::Result<Option<ResourceResponse>> {
    prepare_existing_lockfile_with_registry(
        worktree,
        response_dir,
        npm_cache,
        &response_dir.join("npm-index-snapshots"),
        downloaded_bytes,
        None,
    )
}

fn prepare_existing_lockfile_with_registry(
    worktree: &Path,
    response_dir: &Path,
    npm_cache: &Path,
    npm_snapshot: &Path,
    downloaded_bytes: &AtomicUsize,
    authorized_registry: Option<&url::Url>,
) -> anyhow::Result<Option<ResourceResponse>> {
    let packages = super::lockfile::collect_lockfiles_if_present(worktree)?;
    let has_root_lockfile = super::lockfile::has_root_lockfile(worktree)?;
    if !has_root_lockfile && super::manifest::has_declared_dependencies(worktree)? {
        return Ok(Some(ResourceResponse::needs_attention(
            "Kool.ad/e cannot safely retry this npm addition because the project already declares npm dependencies but has no root package-lock.json or npm-shrinkwrap.json. Create a lockfile for the existing dependencies, then resume; no offline retry ran.".into(),
        )));
    }
    if packages.is_empty() && !has_root_lockfile {
        return Ok(None);
    }
    let purpose = "Prepare existing lockfile packages before adding an npm dependency";
    match authorized_registry {
        Some(registry) => super::prepare_for_addition(
            worktree,
            response_dir,
            npm_cache,
            npm_snapshot,
            registry,
            purpose,
            downloaded_bytes,
        ),
        None => super::prepare(
            worktree,
            response_dir,
            npm_cache,
            npm_snapshot,
            purpose,
            downloaded_bytes,
        ),
    }
    .map(Some)
}

struct TemporaryProject(PathBuf);

impl TemporaryProject {
    fn create(parent: &Path) -> anyhow::Result<Self> {
        let parent = parent.canonicalize()?;
        let path = parent.join(format!("npm-resolution-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path)?;
        crate::harness::resource_bridge::set_private_dir(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
