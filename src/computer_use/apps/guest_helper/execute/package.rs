use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        net::UnixStream,
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};

use crate::computer_use::{
    ComputerActionActorV1, ComputerActionPayloadV1, ComputerActionV1, ComputerSessionClassV1,
    apps::GuestAppOperationKindV1,
};

use super::super::protocol::ReplyV1;
use super::catalog::{helper_config, verified_catalog};

mod cache;
mod operations;
pub(super) use self::cache::*;
use self::operations::*;

const PACKAGE_CACHE: &str = "/var/cache/koolade/apps";
const MAX_PACKAGE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_FREE_SPACE_MULTIPLIER: u64 = 3;
const MIN_FREE_SPACE: u64 = 128 * 1024 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_DPKG_ERROR_BYTES: usize = 16 * 1024;

pub(super) fn manage(action: &ComputerActionV1, stream: &mut UnixStream) -> ReplyV1 {
    if action.schema_version != 1
        || action.actor != ComputerActionActorV1::Human
        || action.operation != crate::computer_use::ComputerOperationV1::ManageApp
        || action.action_sequence == 0
        || !canonical_uuid(&action.idempotency_key)
        || action.lease_generation == 0
        || action.approval_generation == 0
        || action.cancellation_generation == 0
    {
        return ReplyV1::Unsupported;
    }
    let ComputerActionPayloadV1::ManageApp {
        app_id,
        operation,
        catalog_digest,
        image_digest,
        manifest_sha256,
        package_sha256,
    } = &action.payload
    else {
        return ReplyV1::Unsupported;
    };
    let Ok(config) = helper_config() else {
        return ReplyV1::Unsupported;
    };
    if config.session_class != ComputerSessionClassV1::Disposable
        || !config.verifies_action(
            action,
            crate::computer_use::ComputerActionHelperPhaseV1::Manage,
        )
        || action.session_id != config.session_id
        || action.lease_generation != config.lease_generation
        || action.approval_generation != config.approval_generation
        || action.cancellation_generation != config.cancellation_generation
        || action.action_sequence <= config.initial_action_sequence
        || !config.allowed_apps.contains(app_id)
        || config.expires_at_unix_ms <= now_unix_ms()
        || *image_digest != config.image_digest
        || !super::task_stage::consume_nonce(action, "manage")
    {
        return ReplyV1::Unsupported;
    }
    let Ok(catalog) = verified_catalog() else {
        return ReplyV1::Unsupported;
    };
    if catalog.digest() != catalog_digest {
        return ReplyV1::Unsupported;
    }
    let Some(manifest) = catalog.app(app_id) else {
        return ReplyV1::Unsupported;
    };
    let Ok(manifest_bytes) = serde_json::to_vec(manifest) else {
        return ReplyV1::Unsupported;
    };
    let expected_manifest_digest = format!("sha256:{:x}", Sha256::digest(&manifest_bytes));
    if expected_manifest_digest != *manifest_sha256
        || manifest.package.package_sha256 != *package_sha256
        || manifest.required_capabilities.iter().any(|capability| {
            *capability != crate::computer_use::apps::GuestAppCapabilityV1::GraphicalDisplay
        })
    {
        return ReplyV1::Unsupported;
    }
    operate(manifest, *operation, stream)
}

pub(super) fn cached_package_is_valid(digest: &str) -> bool {
    let Some(path) = cache_path(digest) else {
        return false;
    };
    verify_cached_file(&path, digest, Some(0)).is_ok()
}

fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

use std::os::fd::AsRawFd;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_quota_is_bounded_with_rollback_headroom() {
        assert!(has_operation_space(10 * 1024 * 1024, 200 * 1024 * 1024));
        assert!(!has_operation_space(10 * 1024 * 1024, 150 * 1024 * 1024));
        assert!(!has_operation_space(u64::MAX, u64::MAX));
    }

    #[test]
    fn content_addressed_cache_rejects_tampered_bytes_and_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("package.deb");
        std::fs::write(&file, b"trusted package bytes").unwrap();
        std::fs::set_permissions(&file, std::os::unix::fs::PermissionsExt::from_mode(0o600))
            .unwrap();
        let digest = format!("sha256:{:x}", Sha256::digest(b"trusted package bytes"));
        assert!(verify_cached_file(&file, &digest, None).is_ok());
        assert!(verify_cached_file(&file, &format!("sha256:{}", "a".repeat(64)), None).is_err());
        let link = temp.path().join("link.deb");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(verify_cached_file(&link, &digest, None).is_err());
    }

    #[test]
    fn update_requires_a_signed_rollback_package() {
        assert!(!rollback_pair_present(None, None));
        assert!(!rollback_pair_present(Some("0.5.9-1"), None));
        assert!(!rollback_pair_present(
            None,
            Some("sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee")
        ));
        assert!(rollback_pair_present(
            Some("0.5.9-1"),
            Some("sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee")
        ));
    }

    #[test]
    fn package_failures_report_missing_dependencies_and_rollback_failure() {
        assert_eq!(
            classify_dpkg_failure(b"dpkg: dependency problems prevent configuration"),
            DpkgOutcome::MissingDependencies
        );
        assert_eq!(
            classify_dpkg_failure(b"dpkg: error processing package"),
            DpkgOutcome::Failed
        );
        assert_eq!(
            rollback_outcome(DpkgOutcome::MissingDependencies, DpkgOutcome::Completed),
            ReplyV1::MissingDependencies
        );
        assert_eq!(
            rollback_outcome(DpkgOutcome::Failed, DpkgOutcome::Failed),
            ReplyV1::Failed
        );
    }
}
