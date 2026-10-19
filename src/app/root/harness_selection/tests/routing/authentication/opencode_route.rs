use super::*;
use std::os::unix::fs::PermissionsExt;

/// Crosses persisted task routing and the OpenCode availability check. It proves a current
/// authenticated snapshot admits OpenCode; a synthetic CLI and settings file are its only
/// dependencies, with one short version process.
#[test]
fn current_authenticated_opencode_snapshot_is_accepted_by_persisted_route() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let settings = settings_with("opencode", ReadinessState::Ready, now);
    let root =
        std::env::temp_dir().join(format!("harness-opencode-route-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("opencode");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'opencode 1.2.3'; exit 0; fi\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::opencode_harness::OPENCODE_BINARY_ENV,
        binary.as_os_str(),
    );
    let settings_path = root.join("settings.json");

    crate::persistence::harness_settings::with_settings_path(settings_path, || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let mut override_harness = None;
        let harness = configured_harness_for_task(
            &mut override_harness,
            Some(IMPLEMENTATION),
            &std::collections::BTreeMap::new(),
        );
        assert!(harness.check_available().is_ok());
    });
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn unknown_opencode_authentication_snapshot_blocks_persisted_route() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let settings = settings_with("opencode", ReadinessState::Unknown, now);
    let path = std::env::temp_dir().join(format!(
        "harness-opencode-route-unknown-{}.json",
        uuid::Uuid::new_v4()
    ));
    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let mut override_harness = None;
        let harness = configured_harness_for_task(
            &mut override_harness,
            Some(IMPLEMENTATION),
            &std::collections::BTreeMap::new(),
        );
        let error = harness.check_available().unwrap_err().detail();
        assert!(
            error.contains("authenticated capability snapshot"),
            "{error}"
        );
    });
    let _ = std::fs::remove_file(path);
}
