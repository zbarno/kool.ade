//! These tests cross the provider probe's real subprocess boundary with synthetic CLIs.
//! They verify catalog failures preserve the version probe and identify authentication
//! separately, behavior a parser unit test cannot establish. They require Unix `/bin/sh`
//! and temporary files only.

use super::super::*;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

#[test]
fn nonzero_and_empty_catalogs_preserve_installed_readiness() {
    for (name, model_command, expected) in [
        (
            "nonzero",
            "echo 'provider temporarily unavailable' >&2; exit 9",
            "model catalog unavailable",
        ),
        ("empty", "exit 0", "returned no models"),
    ] {
        let root = temp_dir(name);
        let cli = fake_discovery_cli(
            &root,
            &format!(
                "if [ \"$1\" = \"--version\" ]; then echo 'agy 1.2.3'; exit 0; fi\nif [ \"$1\" = \"models\" ]; then {model_command}; fi\nexit 4"
            ),
        );
        let report = probe(&cli, Duration::from_secs(1)).unwrap();
        assert_eq!(report.version, "1.2.3");
        assert_eq!(report.models, Vec::<String>::new());
        assert!(!report.configuration_required);
        assert!(
            report
                .catalog_diagnostic
                .as_deref()
                .unwrap()
                .to_ascii_lowercase()
                .contains(expected)
        );
        let _environment = super::set_binary(&cli);
        let visible = AntigravityHarness::probe_report();
        assert!(visible.ready);
        assert!(!visible.configuration_required);
        assert_eq!(visible.version.as_deref(), Some("1.2.3"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn authentication_failure_keeps_installation_identity_and_is_explicit() {
    let root = temp_dir("authentication");
    let cli = fake_discovery_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo 'agy 2.3.4'; exit 0; fi\necho 'Authentication required' >&2\nexit 1",
    );
    let _environment = super::set_binary(&cli);
    let report = AntigravityHarness::probe_report();
    assert_eq!(report.version.as_deref(), Some("2.3.4"));
    assert_eq!(report.binary.as_deref(), Some(cli.as_path()));
    assert!(!report.ready);
    assert!(report.configuration_required);
    assert_eq!(report.status, "Antigravity (authentication required)");
    assert!(
        report
            .diagnostic
            .to_ascii_lowercase()
            .contains("authentication is required")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn successful_exit_with_authentication_diagnostic_does_not_verify_auth() {
    let root = temp_dir("authentication-success-exit");
    let cli = fake_discovery_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo 'agy 2.4.0'; exit 0; fi\necho 'model-a'\necho 'Authentication required' >&2\nexit 0",
    );
    let report = probe(&cli, Duration::from_secs(1)).unwrap();
    assert!(report.models.is_empty());
    assert!(report.configuration_required);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn timed_out_catalog_probe_is_optional_and_finite() {
    let root = temp_dir("timeout");
    let cli = fake_discovery_cli(
        &root,
        "if [ \"$1\" = \"--version\" ]; then echo 'agy 3.4.5'; exit 0; fi\nsleep 5",
    );
    let started = std::time::Instant::now();
    let report =
        super::super::probe_with_timeouts(&cli, Duration::from_secs(2), Duration::from_millis(100))
            .unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(report.version, "3.4.5");
    assert!(!report.configuration_required);
    assert!(report.catalog_diagnostic.is_some());
    std::fs::remove_dir_all(root).unwrap();
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "koolade-antigravity-catalog-{name}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn fake_discovery_cli(root: &std::path::Path, body: &str) -> std::path::PathBuf {
    let path = root.join("agy");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}
