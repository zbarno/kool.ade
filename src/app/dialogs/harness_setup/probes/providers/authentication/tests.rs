use super::*;
use std::os::unix::fs::PermissionsExt;

/// Production boundary: the Pi authentication probe must not start its credential resolver.
/// Pi can execute a user-defined command while resolving `auth.json`; a fake executable and
/// marker prove this path makes no child process call. No external services are needed.
#[test]
fn pi_authentication_probe_does_not_start_a_potential_host_command() {
    let root = std::env::temp_dir().join(format!("koolade-pi-auth-probe-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let marker = root.join("auth-probe-started");
    let binary = root.join("pi");
    std::fs::write(
        &binary,
        format!("#!/bin/sh\nprintf started > '{}'\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert_eq!(
        pi_authentication(&binary, Some("synthetic-provider")),
        ReadinessState::Unknown
    );
    assert!(!marker.exists());

    std::fs::remove_dir_all(root).unwrap();
}

/// Production boundary: Pi discovery, local credential resolution, result conversion, and
/// persisted readiness. Parser-only coverage cannot prove local credentials stay unverified;
/// a fake CLI and temporary home are the only dependencies, with a bounded subprocess setup.
#[test]
fn pi_local_credentials_and_models_do_not_prove_account_access() {
    let root = std::env::temp_dir().join(format!(
        "koolade-pi-authenticated-provider-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(root.join(".pi/agent")).unwrap();
    std::fs::write(
        root.join(".pi/agent/settings.json"),
        r#"{"defaultProvider":"synthetic","defaultModel":"model-a"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join(".pi/agent/models.json"),
        r#"{"providers":{"synthetic":{"models":[{"id":"model-a"}]}}}"#,
    )
    .unwrap();
    let binary = root.join("pi");
    std::fs::write(
        &binary,
        "#!/bin/sh\ncase \"$1\" in\n--version) echo 'pi 1.2.3'; exit 0;;\n--help) printf '%s\\n' '--print' '--mode' '--no-session' '--no-approve' '--append-system-prompt' '--thinking' '--provider' '--model' '--no-extensions' '--no-skills' '--no-prompt-templates' '--no-themes' '--no-context-files' '--no-tools' '--no-builtin-tools' '--tools' '--extension json' 'json' 'xhigh'; exit 0;;\nauth) echo '{\"status\":\"ready\"}'; exit 0;;\nesac\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("HOME", root.as_os_str());
    environment.remove("PI_CODING_AGENT_DIR");
    environment.set(
        crate::harness::pi_harness::PI_BINARY_ENV,
        binary.as_os_str(),
    );
    let report = super::super::discover_provider("pi", true);
    assert_eq!(report.authentication, ReadinessState::Unknown);
    assert!(!report.model_catalog_fresh);
    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report],
        Some("pi"),
        1,
    );
    let detected = &settings.discovered["pi"];
    assert!(!detected.ready);
    assert_eq!(settings.default_harness, None);
    assert_eq!(
        detected
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        ReadinessState::Unknown
    );
    assert_eq!(
        detected.model_catalog_status(),
        crate::harness::discovery::ModelCatalogStatus::Stale
    );
    assert!(
        !super::super::super::super::has_current_provider_capabilities(
            "pi",
            detected,
            crate::persistence::harness_settings::current_unix_ms()
        )
    );

    std::fs::remove_dir_all(root).unwrap();
}
