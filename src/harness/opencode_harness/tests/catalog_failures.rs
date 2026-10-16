//! These tests cross the provider probe's real subprocess boundary with synthetic CLIs.
//! They prove model-list failures do not erase a successful version probe, which a parser
//! unit test cannot establish. They require Unix `/bin/sh` and temporary files only.

use super::super::*;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

#[test]
fn nonzero_and_empty_catalogs_preserve_installed_readiness() {
    for (name, model_command, readiness, expected, status) in [
        (
            "nonzero",
            "echo 'provider temporarily unavailable' >&2; exit 9",
            OpenCodeReadiness::CatalogUnavailable,
            "model catalog unavailable",
            "opencode 1.2.3 (model catalog unavailable)",
        ),
        (
            "empty",
            "exit 0",
            OpenCodeReadiness::ConfigurationRequired,
            "no configured models",
            "opencode (configuration required)",
        ),
    ] {
        let root = super::temp_dir(name);
        let cli = fake_cli(
            &root,
            &format!(
                "if [ \"$1\" = \"--version\" ]; then echo 'opencode 1.2.3'; exit 0; fi\nif [ \"$2\" = \"models\" ]; then {model_command}; fi\nexit 4"
            ),
        );
        let report = probe_report_for(&cli);
        assert_eq!(report.version.as_deref(), Some("1.2.3"));
        assert_eq!(report.binary.as_deref(), Some(cli.as_path()));
        assert_eq!(report.readiness, readiness);
        assert_eq!(report.status, status);
        assert!(report.diagnostic.to_ascii_lowercase().contains(expected));
        assert!(report.models.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn authentication_failure_is_distinct_and_keeps_installation_identity() {
    let root = super::temp_dir("authentication");
    let cli = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo 'opencode 2.3.4'; exit 0; fi\necho 'Authentication required' >&2\nexit 1",
    );
    let report = probe_report_for(&cli);
    assert_eq!(report.version.as_deref(), Some("2.3.4"));
    assert_eq!(report.readiness, OpenCodeReadiness::AuthenticationRequired);
    assert_eq!(report.status, "opencode (authentication required)");
    assert!(report.diagnostic.contains("auth login"));
    assert!(!report.diagnostic.contains("Authentication required"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn timed_out_catalog_probe_is_optional_and_finite() {
    let root = super::temp_dir("timeout");
    let cli = fake_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo 'opencode 3.4.5'; exit 0; fi\nsleep 5",
    );
    let started = std::time::Instant::now();
    let report = super::super::readiness::probe_with_timeouts(
        &cli,
        Duration::from_secs(2),
        Duration::from_millis(100),
    )
    .unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(report.version, "3.4.5");
    assert_eq!(report.readiness, OpenCodeReadiness::CatalogUnavailable);
    assert!(
        report
            .diagnostic
            .to_ascii_lowercase()
            .contains("catalog unavailable")
    );
    std::fs::remove_dir_all(root).unwrap();
}

fn probe_report_for(path: &std::path::Path) -> OpenCodeProbeReport {
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(OPENCODE_BINARY_ENV, path.as_os_str());
    OpenCodeHarness::probe_report()
}

fn fake_cli(root: &std::path::Path, body: &str) -> std::path::PathBuf {
    let path = root.join("opencode");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}
