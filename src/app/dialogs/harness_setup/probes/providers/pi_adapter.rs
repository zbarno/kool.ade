use super::super::discover_provider;
use crate::execution::contract::ReadinessState;
use std::os::unix::fs::PermissionsExt;

/// The production Pi adapter consumes settings plus a CLI auth probe. This fake executable and
/// temporary config prove malformed model text is removed across discovery and a durable settings
/// save/reload; one short CLI and one temporary file transaction.
#[test]
fn pi_configured_model_output_cannot_persist_secret_text() {
    let root =
        std::env::temp_dir().join(format!("koolade-pi-model-secret-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("settings.json"),
        r#"{"defaultProvider":"synthetic","defaultModel":"gpt-5 token=synthetic-secret"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("models.json"),
        r#"{"providers":{"synthetic":{"models":[{"id":"gpt-5 token=synthetic-secret"}]}}}"#,
    )
    .unwrap();
    let binary = root.join("pi");
    std::fs::write(
        &binary,
        "#!/bin/sh\ncase \"$1\" in\n--version) echo 'pi - v1.2.3' ;;\n--help) printf '%s\\n' '--print' '--mode' '--no-session' '--no-approve' '--append-system-prompt' '--thinking' '--provider' '--model' '--no-extensions' '--no-skills' '--no-prompt-templates' '--no-themes' '--no-context-files' '--no-tools' '--no-builtin-tools' '--tools' '--extension' 'json' 'xhigh' ;;\nauth) echo '{\"status\":\"ready\"}' ;;\n*) exit 1 ;;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::pi_harness::PI_BINARY_ENV,
        binary.as_os_str(),
    );
    environment.set("PI_CODING_AGENT_DIR", root.as_os_str());
    environment.set("HOME", root.as_os_str());
    environment.set("KOOLADE_HOME", root.join("isolated-state").as_os_str());

    let report = discover_provider("pi", true);
    // The live auth check verifies the provider account, but the bounds layer
    // still quarantines model text shaped like credentials; the ready path must
    // carry the quarantine verdict, not the raw text, and forfeit freshness.
    assert_eq!(report.authentication, ReadinessState::Ready);
    assert!(!report.model_catalog_fresh);
    assert!(report.models.is_empty());
    assert!(report.default_model.is_none());
    assert!(!report.status.contains("synthetic-secret"));
    assert!(
        report
            .diagnostic
            .iter()
            .all(|detail| !detail.contains("synthetic-secret"))
    );
    let settings_path = root.join("state/harness-settings.json");
    crate::persistence::harness_settings::with_settings_path(settings_path.clone(), || {
        let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
        super::super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);
        crate::persistence::harness_settings::save(&settings).unwrap();
        let persisted = crate::persistence::harness_settings::load().0;
        let detected = &persisted.discovered["pi"];
        assert!(detected.models.is_empty());
        assert!(detected.default_model.is_none());
        assert!(!detected.model_catalog_fresh);
        assert!(!detected.status.contains("synthetic-secret"));
        let raw = std::fs::read_to_string(settings_path).expect("durable settings file readable");
        assert!(
            !raw.contains("synthetic-secret"),
            "secret text escaped into durable state"
        );
    });
    std::fs::remove_dir_all(root).unwrap();
}
