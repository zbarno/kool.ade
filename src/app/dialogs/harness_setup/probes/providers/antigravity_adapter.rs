//! Exercise Antigravity's real probe-to-settings boundary with a synthetic CLI. The invariant
//! is that an explicit auth diagnostic remains installed but unavailable for selection; the
//! parser test alone cannot verify adapter persistence. Requires Unix shell and temporary files;
//! setup cost is one short provider probe.

use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn explicit_auth_failure_maps_to_unavailable_and_blocks_selection() {
    let root = std::env::temp_dir().join(format!(
        "koolade-antigravity-auth-failure-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("agy");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'agy 1.2.3 token=synthetic-secret'; exit 0; fi\nif [ \"$1\" = \"models\" ]; then echo 'Authentication required' >&2; exit 1; fi\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::antigravity_harness::ANTIGRAVITY_BINARY_ENV,
        binary.as_os_str(),
    );
    let report = discover_provider("antigravity", true);

    assert!(!report.ready);
    assert!(report.configuration_required);
    assert_eq!(
        report.authentication,
        crate::execution::contract::ReadinessState::Unavailable
    );
    assert_eq!(report.version.as_deref(), Some("1.2.3"));
    assert!(!report.status.contains("synthetic-secret"));
    assert!(
        !report
            .diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("synthetic-secret")
    );
    assert_eq!(
        report.executable.as_deref(),
        Some(binary.to_string_lossy().as_ref())
    );

    let result = super::super::super::super::probe_result::from_harness_probe(&report, 1, 10, 1);
    assert_eq!(
        result.status,
        crate::harness::discovery::ProbeStatus::NeedsAuthentication
    );
    assert_eq!(
        result.authentication,
        crate::execution::contract::ReadinessState::Unavailable
    );
    assert_eq!(
        result.execution_capability.safe_for_implementation,
        crate::execution::contract::ReadinessState::Unavailable
    );
    assert_eq!(result.version.as_deref(), Some("1.2.3"));
    assert!(!result.display_status.contains("synthetic-secret"));
    assert!(
        !result
            .diagnostic_message
            .as_deref()
            .unwrap_or_default()
            .contains("synthetic-secret")
    );

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report],
        Some("antigravity"),
        1,
    );
    let detected = &settings.discovered["antigravity"];
    assert!(!detected.ready);
    assert_eq!(detected.version.as_deref(), Some("1.2.3"));
    assert!(!detected.status.contains("synthetic-secret"));
    assert!(
        !detected
            .diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("synthetic-secret")
    );
    assert!(!detected.implementation_available);
    assert!(settings.default_harness.is_none());
    assert_eq!(
        detected
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        crate::execution::contract::ReadinessState::Unavailable
    );

    std::fs::remove_dir_all(root).unwrap();
}
