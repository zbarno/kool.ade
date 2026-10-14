use super::smoke::smoke_test_body;
use crate::execution::docker::images::{ImageLock, RuntimeProfile};

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
fn smoke_body_rejects_unlocked_image_profiles_and_unscoped_names() {
    let lock = ImageLock::bundled().unwrap();
    let rust = lock.select(RuntimeProfile::Rust, "amd64").unwrap();
    assert!(smoke_test_body(&rust, "koolade-smoke-0123456789abcdef0123456789abcdef").is_err());
    let general = lock.select(RuntimeProfile::General, "amd64").unwrap();
    assert!(smoke_test_body(&general, "../koolade-smoke-unsafe").is_err());
}
