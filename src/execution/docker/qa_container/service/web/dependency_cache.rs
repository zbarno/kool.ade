use super::super::super::QaWebPreviewSpec;
use crate::execution::{contract::DependencyCacheEcosystemV1, errors::ExecutionError};
use std::{collections::HashMap, io::Read};

pub(super) fn matches_request(spec: &QaWebPreviewSpec<'_>) -> bool {
    let mut requested_refs = spec
        .request
        .dependency_cache_refs
        .iter()
        .map(|dependency| dependency.cache_ref.clone())
        .collect::<Vec<_>>();
    let mut transferred_refs = spec
        .dependency_caches
        .iter()
        .map(|snapshot| snapshot.cache_ref.clone())
        .collect::<Vec<_>>();
    requested_refs.sort();
    transferred_refs.sort();
    let total_bytes = spec
        .dependency_caches
        .iter()
        .map(|snapshot| snapshot.archive.len() as u64)
        .sum::<u64>();
    requested_refs == transferred_refs
        && total_bytes <= spec.resources.temporary_disk_bytes
        && spec.dependency_caches.iter().all(|snapshot| {
            snapshot
                .manifest
                .validate_ref(&snapshot.cache_ref, &snapshot.archive)
                .is_ok()
                && snapshot.cache_ref.target_arch
                    == spec
                        .browser_image
                        .platform
                        .strip_prefix("linux/")
                        .unwrap_or_default()
        })
}

pub(super) fn service_tmpfs(
    spec: &QaWebPreviewSpec<'_>,
    env: &mut Vec<String>,
) -> Result<HashMap<String, String>, ExecutionError> {
    let (uid, gid) = runtime_uid_gid(&spec.service_image.runtime_user)?;
    let mut tmpfs = HashMap::from([
        (
            "/tmp".into(),
            format!(
                "rw,nosuid,nodev,noexec,size={},uid={uid},gid={gid}",
                spec.resources.temporary_disk_bytes / 2,
            ),
        ),
        (
            "/workspace".into(),
            format!(
                "rw,nosuid,nodev,size={},uid={uid},gid={gid}",
                spec.resources.temporary_disk_bytes / 2,
            ),
        ),
    ]);
    if spec.dependency_caches.is_empty() {
        return Ok(tmpfs);
    }
    let cache_mount = format!(
        "rw,nosuid,nodev,noexec,size={},uid={uid},gid={gid}",
        spec.resources.temporary_disk_bytes / 4
    );
    let mut has_npm = false;
    let mut has_cargo = false;
    for cache in spec.dependency_caches {
        match cache.cache_ref.ecosystem {
            DependencyCacheEcosystemV1::Npm => has_npm = true,
            DependencyCacheEcosystemV1::Cargo => has_cargo = true,
        }
    }
    if has_npm {
        tmpfs.insert("/home/executor/.npm".into(), cache_mount.clone());
        env.extend([
            "NPM_CONFIG_CACHE=/home/executor/.npm".into(),
            "NPM_CONFIG_OFFLINE=true".into(),
        ]);
    }
    if has_cargo {
        tmpfs.insert("/home/executor/.cargo".into(), cache_mount);
        env.extend([
            "CARGO_HOME=/home/executor/.cargo".into(),
            "CARGO_NET_OFFLINE=true".into(),
        ]);
    }
    Ok(tmpfs)
}

fn runtime_uid_gid(runtime_user: &str) -> Result<(&str, &str), ExecutionError> {
    let (uid, gid) = runtime_user
        .split_once(':')
        .ok_or_else(|| super::super::super::denied("QA service image runtime user is invalid"))?;
    if uid.is_empty()
        || gid.is_empty()
        || !uid.bytes().all(|byte| byte.is_ascii_digit())
        || !gid.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(super::super::super::denied(
            "QA service image runtime user is invalid",
        ));
    }
    Ok((uid, gid))
}

pub(super) fn archive_for_runtime_user(
    snapshot: &crate::harness::resource_bridge::CacheSnapshot,
    runtime_user: &str,
) -> Result<Vec<u8>, ExecutionError> {
    snapshot
        .manifest
        .validate_ref(&snapshot.cache_ref, &snapshot.archive)
        .map_err(|_| super::super::super::denied("QA dependency cache snapshot is invalid"))?;
    let (uid, gid) = runtime_uid_gid(runtime_user)?;
    let uid = uid
        .parse::<u64>()
        .map_err(|_| super::super::super::denied("QA service image runtime user is invalid"))?;
    let gid = gid
        .parse::<u64>()
        .map_err(|_| super::super::super::denied("QA service image runtime user is invalid"))?;
    let mut output = Vec::with_capacity(snapshot.archive.len());
    let mut source = tar::Archive::new(snapshot.archive.as_slice());
    let mut target = tar::Builder::new(&mut output);
    target.mode(tar::HeaderMode::Deterministic);
    for entry in source
        .entries()
        .map_err(|_| super::super::super::denied("QA dependency cache archive is invalid"))?
    {
        let mut entry = entry
            .map_err(|_| super::super::super::denied("QA dependency cache archive is invalid"))?;
        let path = entry
            .path()
            .map_err(|_| super::super::super::denied("QA dependency cache archive is invalid"))?
            .into_owned();
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut bytes)
            .map_err(|_| super::super::super::denied("QA dependency cache archive is invalid"))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o600);
        header.set_uid(uid);
        header.set_gid(gid);
        header.set_mtime(0);
        header.set_cksum();
        target
            .append_data(&mut header, path, bytes.as_slice())
            .map_err(|_| super::super::super::denied("QA dependency cache archive is invalid"))?;
    }
    target
        .finish()
        .map_err(|_| super::super::super::denied("QA dependency cache archive is invalid"))?;
    drop(target);
    Ok(output)
}
