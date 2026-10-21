use super::{DockerExecutionBackend, internal_error_with_context};
use crate::execution::{
    docker::qa_container::{self, QaWebPreview, QaWebPreviewSpec},
    errors::ExecutionError,
};
use std::{
    collections::BTreeSet,
    io::{Cursor, Read},
    path::Path,
};

pub(super) async fn start(
    backend: &DockerExecutionBackend,
    spec: QaWebPreviewSpec<'_>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) -> Result<QaWebPreview, ExecutionError> {
    let cache_uploads = {
        let caches = backend.dependency_caches.lock().map_err(|_| {
            internal_error_with_context("reading authorized Runtime QA dependency caches")
        })?;
        let mut uploads = Vec::with_capacity(spec.dependency_cache_refs.len());
        let mut total = 0_u64;
        for cache_ref in spec.dependency_cache_refs {
            let snapshot = caches.get(cache_ref).ok_or_else(|| {
                internal_error_with_context("resolving an authorized Runtime QA cache snapshot")
            })?;
            snapshot
                .manifest
                .validate_ref(cache_ref, &snapshot.archive)
                .map_err(|_| {
                    internal_error_with_context(
                        "validating an authorized Runtime QA cache snapshot",
                    )
                })?;
            if cache_ref.task_id != spec.request.parent_task_uid
                || cache_ref.target_id != "local"
                || cache_ref.target_arch
                    != spec
                        .service_image
                        .platform
                        .strip_prefix("linux/")
                        .unwrap_or_default()
            {
                return Err(internal_error_with_context(
                    "checking Runtime QA cache scope",
                ));
            }
            total = total.saturating_add(snapshot.archive.len() as u64);
            if total > crate::harness::resource_bridge::MAX_CACHE_SNAPSHOT_BYTES {
                return Err(internal_error_with_context(
                    "bounding Runtime QA cache uploads",
                ));
            }
            let archive = service_cache_archive(snapshot, &spec.service_image.runtime_user)?;
            uploads.push(crate::execution::docker::qa_container::QaContainerUpload {
                // `/tmp` is present on the service tmpfs before startup;
                // Docker's archive API does not need to create a cache root.
                target: "/tmp".into(),
                archive,
            });
        }
        uploads
    };
    qa_container::start_web_preview(&backend.docker, spec, &cache_uploads, cancel).await
}

fn service_cache_archive(
    snapshot: &crate::harness::resource_bridge::CacheSnapshot,
    runtime_user: &str,
) -> Result<Vec<u8>, ExecutionError> {
    let uid = runtime_user
        .split_once(':')
        .map_or(runtime_user, |(uid, _)| uid)
        .parse::<u64>()
        .ok()
        .filter(|uid| *uid > 0)
        .ok_or_else(|| internal_error_with_context("validating the QA service user"))?;
    let expected_paths = snapshot
        .manifest
        .entries
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<BTreeSet<_>>();
    let cache_root = match snapshot.manifest.ecosystem {
        crate::execution::contract::DependencyCacheEcosystemV1::Npm => ".npm",
        crate::execution::contract::DependencyCacheEcosystemV1::Cargo => ".cargo",
    };
    let mut files = Vec::with_capacity(expected_paths.len());
    let mut directories = BTreeSet::new();
    let mut archive = tar::Archive::new(Cursor::new(&snapshot.archive));
    for entry in archive
        .entries()
        .map_err(|_| internal_error_with_context("reading the verified QA cache archive"))?
    {
        let mut entry =
            entry.map_err(|_| internal_error_with_context("reading a verified QA cache file"))?;
        let path = entry
            .path()
            .map_err(|_| internal_error_with_context("validating a QA cache file path"))?
            .to_str()
            .ok_or_else(|| internal_error_with_context("validating a QA cache file path"))?
            .to_owned();
        if !entry.header().entry_type().is_file() || !expected_paths.contains(path.as_str()) {
            return Err(internal_error_with_context(
                "checking verified QA cache archive entries",
            ));
        }
        let prefixed_path = format!("{cache_root}/{path}");
        let mut parent = Path::new(&prefixed_path).parent();
        while let Some(directory) = parent {
            if directory.as_os_str().is_empty() {
                break;
            }
            directories.insert(directory.to_string_lossy().into_owned());
            parent = directory.parent();
        }
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|_| internal_error_with_context("reading a verified QA cache file"))?;
        files.push((prefixed_path, bytes));
    }
    if files.len() != expected_paths.len() {
        return Err(internal_error_with_context(
            "checking verified QA cache archive coverage",
        ));
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut output = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut output);
        builder.mode(tar::HeaderMode::Deterministic);
        for directory in directories {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            header.set_mode(0o700);
            header.set_uid(uid);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_cksum();
            builder
                .append_data(&mut header, directory, std::io::empty())
                .map_err(|_| internal_error_with_context("encoding QA cache directories"))?;
        }
        for (path, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_size(bytes.len() as u64);
            header.set_mode(0o600);
            header.set_uid(uid);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_cksum();
            builder
                .append_data(&mut header, path, bytes.as_slice())
                .map_err(|_| internal_error_with_context("encoding QA cache files"))?;
        }
        builder
            .finish()
            .map_err(|_| internal_error_with_context("finalizing the QA cache archive"))?;
    }
    if output.len() as u64 > crate::harness::resource_bridge::MAX_CACHE_SNAPSHOT_BYTES {
        return Err(internal_error_with_context("bounding the QA cache archive"));
    }
    Ok(output)
}

pub(super) async fn cleanup(
    backend: &DockerExecutionBackend,
    lease: &crate::runtime_qa::QaTaskServiceLeaseV1,
    binding: &crate::runtime_qa::QaPreviewRelayBindingV1,
) -> Result<(), ExecutionError> {
    if lease.task_network_id.is_empty() || binding.relay_browser_network_id.is_empty() {
        return Err(internal_error_with_context(
            "validating the QA preview lease and relay binding",
        ));
    }
    qa_container::cleanup_web_preview(&backend.docker, lease, binding).await
}

pub(super) async fn recover(
    backend: &DockerExecutionBackend,
    request: &crate::runtime_qa::QaRunRequestV1,
) -> Result<(), ExecutionError> {
    qa_container::recover_owned_web_preview(&backend.docker, request).await
}

#[cfg(test)]
mod tests {
    use super::service_cache_archive;
    use crate::execution::contract::DependencyCacheEcosystemV1;
    use std::{fs, io::Cursor};

    #[test]
    fn service_cache_archive_is_readable_only_by_the_locked_non_root_user() {
        let root = tempfile::tempdir().unwrap();
        let cache_root = root.path().join("npm-cache");
        fs::create_dir_all(cache_root.join("_cacache/data")).unwrap();
        fs::write(
            cache_root.join("_cacache/data/item"),
            b"synthetic package data",
        )
        .unwrap();
        let snapshot = crate::harness::resource_bridge::CacheSnapshot::capture(
            &cache_root,
            "00000000-0000-4000-8000-000000000001",
            DependencyCacheEcosystemV1::Npm,
            "00000000-0000-4000-8000-000000000002",
            "sha256:resolved",
            "x86_64",
            "00000000-0000-4000-8000-000000000003",
        )
        .unwrap();

        let archive = service_cache_archive(&snapshot, "10001:10001").unwrap();
        let mut directories = 0;
        let mut files = 0;
        for entry in tar::Archive::new(Cursor::new(archive)).entries().unwrap() {
            let entry = entry.unwrap();
            assert_eq!(entry.header().uid().unwrap(), 10001);
            match entry.header().entry_type() {
                kind if kind.is_dir() => {
                    assert_eq!(entry.header().mode().unwrap(), 0o700);
                    directories += 1;
                }
                kind if kind.is_file() => {
                    assert_eq!(entry.header().mode().unwrap(), 0o600);
                    files += 1;
                }
                _ => panic!("cache archive contains an unsupported entry type"),
            }
        }
        assert_eq!(directories, 3);
        assert_eq!(files, 1);
    }
}
