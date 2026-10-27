use super::smoke::{smoke_cleanup_target, smoke_test_body};
use crate::execution::docker::images::{ImageLock, RuntimeProfile};

#[test]
fn smoke_cleanup_requires_the_id_returned_by_container_creation() {
    assert_eq!(smoke_cleanup_target(None), None);
    assert_eq!(smoke_cleanup_target(Some("")), None);
    assert_eq!(
        smoke_cleanup_target(Some("created-container-id")),
        Some("created-container-id")
    );
}

#[test]
fn smoke_body_uses_only_pinned_non_root_no_mount_no_network_policy() {
    let image = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "amd64")
        .unwrap();
    let body = smoke_test_body(&image, "koolade-smoke-0123456789abcdef0123456789abcdef").unwrap();
    let config = body.host_config.unwrap();
    assert_eq!(body.user.as_deref(), Some("10001:10001"));
    assert_eq!(body.image.as_deref(), Some(image.reference.as_str()));
    assert_eq!(
        body.entrypoint.as_ref().unwrap(),
        &vec![
            String::from("/bin/sh"),
            String::from("-c"),
            String::from("exit 0"),
        ]
    );
    assert_eq!(config.network_mode.as_deref(), Some("none"));
    assert_eq!(config.privileged, Some(false));
    assert_eq!(config.readonly_rootfs, Some(true));
    assert_eq!(config.cap_drop.as_ref().unwrap(), &vec!["ALL".to_owned()]);
    assert!(config.cap_add.as_ref().unwrap().is_empty());
    assert!(config.mounts.as_ref().unwrap().is_empty());
    assert!(config.binds.as_ref().unwrap().is_empty());
    assert_eq!(
        config.tmpfs.as_ref().map(std::collections::HashMap::len),
        Some(0)
    );
    assert_eq!(config.memory, Some(128 * 1024 * 1024));
    assert_eq!(config.cpu_quota, Some(25_000));
    assert_eq!(config.pids_limit, Some(16));
    assert_eq!(
        config.security_opt.as_ref().unwrap(),
        &vec!["no-new-privileges:true".to_owned()]
    );
}

#[test]
fn smoke_body_accepts_each_enabled_locked_profile_and_rejects_qa_or_unscoped_inputs() {
    let lock = ImageLock::bundled().unwrap();
    for profile in [
        RuntimeProfile::General,
        RuntimeProfile::Rust,
        RuntimeProfile::Node,
        RuntimeProfile::Python,
        RuntimeProfile::Go,
    ] {
        let image = lock.select(profile, "amd64").unwrap();
        assert!(smoke_test_body(&image, "koolade-smoke-0123456789abcdef0123456789abcdef").is_ok());
    }
    let rust = lock.select(RuntimeProfile::Rust, "amd64").unwrap();
    let mut qa = rust.clone();
    qa.profile = RuntimeProfile::BrowserQa;
    assert_eq!(
        smoke_test_body(&qa, "koolade-smoke-0123456789abcdef0123456789abcdef")
            .unwrap_err()
            .code,
        crate::execution::docker::discovery::PrerequisiteCode::ProfileUnsupported
    );
    let mut tampered = rust.clone();
    tampered.digest = format!("sha256:{}", "0".repeat(64));
    assert_eq!(
        smoke_test_body(&tampered, "koolade-smoke-0123456789abcdef0123456789abcdef")
            .unwrap_err()
            .code,
        crate::execution::docker::discovery::PrerequisiteCode::ImageDigestMismatch
    );
    let general = lock.select(RuntimeProfile::General, "amd64").unwrap();
    assert!(smoke_test_body(&general, "../koolade-smoke-unsafe").is_err());
}
