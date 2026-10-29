use std::collections::{BTreeMap, HashMap};

use crate::{
    execution::{
        contract::DependencyCacheEcosystemV1,
        docker::{images::SelectedImage, qa_container::denied},
        errors::ExecutionError,
    },
    harness::resource_bridge::CacheSnapshot,
    runtime_qa::QaRunRequestV1,
};

const CACHE_TMPFS_PAGE_BYTES: u64 = 64 * 1024;
const CACHE_TMPFS_METADATA_SLACK_BYTES: u64 = 1024 * 1024;

pub(super) fn validate_for_driver(
    driver: &str,
    request: &QaRunRequestV1,
    snapshots: &[CacheSnapshot],
    image: &SelectedImage,
    resources: &crate::execution::contract::ResourceBudgetV1,
) -> Result<(), ExecutionError> {
    if driver != "desktop" {
        return if snapshots.is_empty() {
            Ok(())
        } else {
            Err(denied(
                "Only desktop QA driver containers accept dependency caches",
            ))
        };
    }
    let mut requested = request
        .dependency_cache_refs
        .iter()
        .filter(|dependency| {
            crate::execution::docker::discovery::parse_architecture(
                &dependency.cache_ref.target_arch,
            ) == image.platform.strip_prefix("linux/")
        })
        .map(|dependency| dependency.cache_ref.clone())
        .collect::<Vec<_>>();
    let mut transferred = snapshots
        .iter()
        .map(|snapshot| snapshot.cache_ref.clone())
        .collect::<Vec<_>>();
    requested.sort();
    transferred.sort();
    let total_bytes = snapshots
        .iter()
        .map(|snapshot| snapshot.archive.len() as u64)
        .sum::<u64>();
    let architecture = image.platform.strip_prefix("linux/").unwrap_or_default();
    if requested != transferred
        || snapshots.len() > 8
        || total_bytes > resources.temporary_disk_bytes / 2
        || snapshots.iter().any(|snapshot| {
            crate::execution::docker::discovery::parse_architecture(&snapshot.cache_ref.target_arch)
                != Some(architecture)
                || snapshot
                    .manifest
                    .validate_ref(&snapshot.cache_ref, &snapshot.archive)
                    .is_err()
        })
    {
        return Err(denied(
            "Desktop QA dependency caches do not match the immutable request or bounded image architecture",
        ));
    }
    Ok(())
}

pub(super) fn configure_desktop_cache_mounts(
    driver: &str,
    snapshots: &[CacheSnapshot],
    image: &SelectedImage,
    resources: &crate::execution::contract::ResourceBudgetV1,
    tmpfs: &mut HashMap<String, String>,
    env: &mut Vec<String>,
) -> Result<(), ExecutionError> {
    if driver != "desktop" || snapshots.is_empty() {
        return Ok(());
    }
    let (uid, gid) = image
        .runtime_user
        .split_once(':')
        .ok_or_else(|| denied("Desktop QA image runtime user is invalid"))?;
    if !uid.bytes().all(|byte| byte.is_ascii_digit())
        || !gid.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(denied("Desktop QA image runtime user is invalid"));
    }
    let mut cache_sizes = BTreeMap::new();
    for snapshot in snapshots {
        let allocated_bytes = snapshot
            .manifest
            .entries
            .iter()
            .try_fold(0_u64, |total, entry| {
                let rounded = entry.size_bytes.checked_add(CACHE_TMPFS_PAGE_BYTES - 1)?
                    / CACHE_TMPFS_PAGE_BYTES
                    * CACHE_TMPFS_PAGE_BYTES;
                total.checked_add(rounded)
            })
            .ok_or_else(|| denied("Desktop QA dependency cache size is outside its limit"))?;
        let size = cache_sizes
            .entry(snapshot.cache_ref.ecosystem)
            .or_insert(0_u64);
        *size = size
            .checked_add(allocated_bytes)
            .ok_or_else(|| denied("Desktop QA dependency cache size is outside its limit"))?;
    }
    let cache_sizes = cache_sizes
        .into_iter()
        .map(|(ecosystem, size)| {
            size.checked_add(CACHE_TMPFS_METADATA_SLACK_BYTES)
                .map(|size| (ecosystem, size))
                .ok_or_else(|| denied("Desktop QA dependency cache size is outside its limit"))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let cache_total = cache_sizes.values().copied().sum::<u64>();
    if cache_total >= resources.temporary_disk_bytes {
        return Err(denied(
            "Desktop QA dependency caches exceed their tmpfs budget",
        ));
    }
    let base_mount_size = (resources.temporary_disk_bytes - cache_total) / 4;
    tmpfs.insert(
        "/tmp".into(),
        format!("rw,nosuid,nodev,noexec,size={base_mount_size},uid={uid},gid={gid},mode=1777"),
    );
    tmpfs.insert(
        "/run".into(),
        format!("rw,nosuid,nodev,noexec,size={base_mount_size},uid={uid},gid={gid},mode=0755"),
    );
    tmpfs.insert(
        "/home/executor".into(),
        format!("rw,nosuid,nodev,noexec,size={base_mount_size},uid={uid},gid={gid},mode=0755"),
    );
    if tmpfs.contains_key("/workspace") {
        tmpfs.insert("/workspace".into(), workspace_tmpfs_mount(base_mount_size));
    }
    for (ecosystem, mount_size) in cache_sizes {
        let mount = format!("rw,nosuid,nodev,noexec,size={mount_size},uid={uid},gid={gid}");
        match ecosystem {
            DependencyCacheEcosystemV1::Npm => {
                tmpfs.insert("/home/executor/.npm".into(), mount.clone());
                env.extend([
                    "NPM_CONFIG_CACHE=/home/executor/.npm".into(),
                    "NPM_CONFIG_OFFLINE=true".into(),
                ]);
            }
            DependencyCacheEcosystemV1::Cargo => {
                tmpfs.insert("/home/executor/.cargo".into(), mount.clone());
                env.extend([
                    "CARGO_HOME=/home/executor/.cargo".into(),
                    "CARGO_NET_OFFLINE=true".into(),
                ]);
            }
            DependencyCacheEcosystemV1::Pip => {
                tmpfs.insert("/home/executor/.cache/koolade/pip".into(), mount.clone());
                env.extend([
                    "PIP_CONFIG_FILE=/dev/null".into(),
                    "PIP_NO_INDEX=1".into(),
                    "PIP_FIND_LINKS=/home/executor/.cache/koolade/pip/wheels".into(),
                ]);
            }
            DependencyCacheEcosystemV1::Go => {
                tmpfs.insert("/home/executor/go".into(), mount.clone());
                env.extend([
                    "GOPATH=/home/executor/go".into(),
                    "GOMODCACHE=/home/executor/go/pkg/mod".into(),
                    "GOPROXY=off".into(),
                    "GOSUMDB=off".into(),
                    "GOTOOLCHAIN=local".into(),
                    "GOENV=off".into(),
                    "GOWORK=off".into(),
                    "GOPRIVATE=".into(),
                    "GONOPROXY=".into(),
                    "GONOSUMDB=".into(),
                    "GOINSECURE=".into(),
                ]);
            }
        }
    }
    Ok(())
}

fn workspace_tmpfs_mount(size: u64) -> String {
    format!("rw,nosuid,nodev,exec,size={size},uid=0,gid=0,mode=0755")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_tmpfs_allowance_rounds_each_file_to_a_conservative_page_size() {
        let sizes = [0_u64, 1, 65_536, 65_537];
        let allocated = sizes
            .iter()
            .map(|size| {
                size.checked_add(CACHE_TMPFS_PAGE_BYTES - 1).unwrap() / CACHE_TMPFS_PAGE_BYTES
                    * CACHE_TMPFS_PAGE_BYTES
            })
            .sum::<u64>();

        assert_eq!(allocated, 4 * CACHE_TMPFS_PAGE_BYTES);
    }

    #[test]
    fn desktop_workspace_tmpfs_is_root_owned_for_read_only_artifact_staging() {
        assert_eq!(
            workspace_tmpfs_mount(4096),
            "rw,nosuid,nodev,exec,size=4096,uid=0,gid=0,mode=0755"
        );
    }
}
