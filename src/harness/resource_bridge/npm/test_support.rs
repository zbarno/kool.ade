use super::*;
use sha2::{Digest, Sha512};
use std::path::PathBuf;

pub(in crate::harness::resource_bridge) fn test_preparation_operations(
    archive: PathBuf,
    registry_url: String,
) -> std::sync::Arc<SharedPreparationOperations> {
    let retrieve = std::sync::Arc::new(
        move |response_dir: &Path,
              _url: &str,
              _purpose: &str,
              _remaining: u64,
              _registry: Option<&url::Url>| {
            let target = response_dir.join("synthetic-npm-package.tgz");
            std::fs::copy(&archive, &target)?;
            let bytes = usize::try_from(std::fs::metadata(&target)?.len())?;
            Ok(ResourceResponse::allowed_file(
                "Retrieved a synthetic lockfile-pinned npm archive".into(),
                target
                    .file_name()
                    .ok_or_else(|| anyhow::anyhow!("Synthetic archive has no filename"))?
                    .to_string_lossy()
                    .into_owned(),
                bytes,
            ))
        },
    );
    let index_cache = std::sync::Arc::new(
        move |worktree: &Path,
              npm_cache: &Path,
              archives: &[PathBuf],
              timeout: std::time::Duration| {
            let npm = cache::locate_npm(worktree)?;
            cache::add_to_cache(&npm, npm_cache, archives, timeout)?;
            for archive in archives {
                cache::add_test_registry_entry(&npm, npm_cache, archive, &registry_url)?;
            }
            Ok(())
        },
    );
    std::sync::Arc::new(SharedPreparationOperations {
        retrieve,
        index_cache,
        resolve_addition: std::sync::Arc::new(|_, _, _, _| {
            anyhow::bail!("Synthetic npm addition resolver was not configured")
        }),
    })
}

pub(in crate::harness::resource_bridge) fn test_addition_preparation_operations(
    archive: PathBuf,
    package_name: String,
    version: String,
    registry_url: String,
) -> std::sync::Arc<SharedPreparationOperations> {
    let archive_url = format!(
        "{}/{package_name}/-/{package_name}-{version}.tgz",
        registry_url.trim_end_matches('/')
    );
    let resolve_addition: std::sync::Arc<super::preparation::NpmAdditionResolver<'static>> = {
        let package_name = package_name.clone();
        let version = version.clone();
        let archive_url = archive_url.clone();
        let registry_url = registry_url.clone();
        let archive = archive.clone();
        std::sync::Arc::new(move |project, npm_cache, package_spec, registry| {
            anyhow::ensure!(
                package_spec == format!("{package_name}@{version}") && registry == registry_url,
                "Synthetic npm resolver received an unexpected package request"
            );
            let archive_bytes = std::fs::read(&archive)?;
            let integrity = format!("sha512-{}", super::base64(&Sha512::digest(&archive_bytes)));
            let package_path = format!("node_modules/{package_name}");
            std::fs::write(
                project.join("package-lock.json"),
                serde_json::to_vec(&serde_json::json!({
                    "name": "koolade-dependency-resolver",
                    "version": "1.0.0",
                    "lockfileVersion": 3,
                    "packages": {
                        "": { "name": "koolade-dependency-resolver", "version": "1.0.0" },
                        (package_path): {
                            "version": version.clone(),
                            "resolved": archive_url.clone(),
                            "integrity": integrity.clone(),
                        }
                    }
                }))?,
            )?;
            let npm = cache::locate_npm(project)?;
            cache::add_test_registry_packument(
                &npm,
                npm_cache,
                &registry_url,
                &package_name,
                &version,
                &archive_url,
                &integrity,
            )
        })
    };
    let retrieve = {
        let archive = archive.clone();
        let archive_url = archive_url.clone();
        std::sync::Arc::new(
            move |response_dir: &Path, url: &str, _: &str, _: u64, _: Option<&url::Url>| {
                anyhow::ensure!(url == archive_url, "Synthetic npm archive URL changed");
                let target = response_dir.join("synthetic-npm-addition.tgz");
                std::fs::copy(&archive, &target)?;
                let bytes = usize::try_from(std::fs::metadata(&target)?.len())?;
                Ok(ResourceResponse::allowed_file(
                    "Retrieved a synthetic lockfile-pinned npm archive".into(),
                    target.file_name().unwrap().to_string_lossy().into_owned(),
                    bytes,
                ))
            },
        )
    };
    let registry_for_index = archive_url.clone();
    let index_cache = std::sync::Arc::new(
        move |worktree: &Path,
              npm_cache: &Path,
              archives: &[PathBuf],
              timeout: std::time::Duration| {
            let npm = cache::locate_npm(worktree)?;
            cache::add_to_cache(&npm, npm_cache, archives, timeout)?;
            for archive in archives {
                cache::add_test_registry_entry(&npm, npm_cache, archive, &registry_for_index)?;
            }
            Ok(())
        },
    );
    std::sync::Arc::new(SharedPreparationOperations {
        retrieve,
        index_cache,
        resolve_addition,
    })
}
