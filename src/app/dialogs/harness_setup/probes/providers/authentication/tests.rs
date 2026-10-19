use super::*;
use std::os::unix::fs::PermissionsExt;

/// Production boundary: supervised Pi CLI auth-check argv/output mapping. A pure parser
/// test cannot prove the CLI's documented command is used; only a synthetic executable and
/// temporary files are required, with one short child process per status.
#[test]
fn pi_auth_check_maps_documented_statuses_without_exposing_credentials() {
    for (body, expected) in [
        (
            "printf '{\"status\":\"ready\"}'; exit 0",
            ReadinessState::Ready,
        ),
        (
            "printf '{\"status\":\"not_ready\"}'; exit 1",
            ReadinessState::Unavailable,
        ),
        (
            "printf '{\"status\":\"invalid\"}'; exit 2",
            ReadinessState::Unavailable,
        ),
        ("printf 'unknown response'", ReadinessState::Unknown),
    ] {
        let root =
            std::env::temp_dir().join(format!("koolade-pi-auth-check-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let binary = root.join("pi");
        std::fs::write(&binary, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(
            pi_authentication(&binary, Some("synthetic-provider")),
            expected
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}

/// Production boundary: successful Pi binary discovery, auth preflight, result conversion, and
/// persisted selection. Parser-only coverage cannot prove these layers agree; a fake CLI and
/// temporary home are the only dependencies, with a small bounded subprocess setup.
#[test]
fn pi_confirmed_authentication_is_selectable() {
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
    assert_eq!(report.authentication, ReadinessState::Ready);
    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report],
        Some("pi"),
        1,
    );
    let detected = &settings.discovered["pi"];
    assert!(detected.ready);
    assert_eq!(settings.default_harness.as_deref(), Some("pi"));
    assert!(
        super::super::super::super::has_current_provider_capabilities(
            "pi",
            detected,
            crate::persistence::harness_settings::current_unix_ms()
        )
    );

    std::fs::remove_dir_all(root).unwrap();
}
