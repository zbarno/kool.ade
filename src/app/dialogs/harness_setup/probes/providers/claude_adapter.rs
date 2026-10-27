use super::super::{ReadinessState, discover_provider};
use std::os::unix::fs::PermissionsExt;

/// Production boundary: a successful `claude auth status` and local model setting cross the CLI
/// adapter into persisted capability state. Parser-only tests cannot prove stale model evidence
/// still blocks routing after login appears successful. Uses one fake Unix CLI and temporary HOME;
/// setup cost is one short probe.
#[cfg(unix)]
#[test]
fn reported_login_does_not_make_a_configured_model_catalog_fresh() {
    let root = std::env::temp_dir().join(format!(
        "koolade-claude-login-model-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(
        root.join(".claude/settings.json"),
        r#"{"model":"synthetic-sonnet"}"#,
    )
    .unwrap();
    let binary = root.join("claude");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--settings\" ]; then [ \"$2\" = '{\"env\":{\"DISABLE_UPDATES\":\"1\"}}' ] || exit 94; shift 2; fi\n[ \"$DISABLE_UPDATES\" = \"1\" ] || exit 93\nif [ \"$1\" = \"--version\" ]; then echo 'Claude Code v2.1.300'; exit 0; fi\nif [ \"$1\" = \"auth\" ]; then echo '{\"authMethod\":\"claude.ai\"}'; exit 0; fi\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::claude_harness::CLAUDE_BINARY_ENV,
        binary.as_os_str(),
    );
    environment.set("HOME", root.as_os_str());
    environment.set("KOOLADE_HOME", root.join("isolated-state").as_os_str());
    environment.remove(crate::harness::claude_harness::CLAUDE_MODEL_ENV);

    let report = discover_provider("claude", true);
    assert!(report.ready);
    assert_eq!(report.authentication, ReadinessState::Ready);
    assert_eq!(report.models, ["synthetic-sonnet"]);
    assert!(!report.model_catalog_fresh);

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);
    let detected = &settings.discovered["claude"];
    assert!(
        detected.ready,
        "the vendor-reported login state is retained"
    );
    assert!(!detected.model_catalog_fresh);
    assert!(detected.has_current_authenticated_capability(
        "claude",
        crate::persistence::harness_settings::current_unix_ms()
    ));
    assert!(settings.default_harness.is_none());
    std::fs::remove_dir_all(root).unwrap();
}

/// Covers the production Claude adapter through typed conversion and settings persistence. This
/// proves auth-required state and configured-but-unverified model evidence survive provider wiring. The fake
/// CLI and temporary HOME are the only dependencies; setup cost is one short CLI.
#[cfg(unix)]
#[test]
fn claude_auth_requirement_does_not_mark_a_configured_model_fresh() {
    let root =
        std::env::temp_dir().join(format!("koolade-claude-adapter-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(
        root.join(".claude/settings.json"),
        r#"{"model":"synthetic-sonnet"}"#,
    )
    .unwrap();
    let binary = root.join("claude");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--settings\" ]; then [ \"$2\" = '{\"env\":{\"DISABLE_UPDATES\":\"1\"}}' ] || exit 94; shift 2; fi\n[ \"$DISABLE_UPDATES\" = \"1\" ] || exit 93\nif [ \"$1\" = \"--version\" ]; then echo '2.1.300'; exit 0; fi\nif [ \"$1\" = \"auth\" ]; then echo '{\"authMethod\":\"none\"}'; exit 0; fi\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::claude_harness::CLAUDE_BINARY_ENV,
        binary.as_os_str(),
    );
    environment.set("HOME", root.as_os_str());
    environment.set("KOOLADE_HOME", root.join("isolated-state").as_os_str());
    environment.remove(crate::harness::claude_harness::CLAUDE_MODEL_ENV);

    let report = discover_provider("claude", true);
    assert!(!report.ready);
    assert_eq!(report.authentication, ReadinessState::Unavailable);
    assert!(report.configuration_required);
    assert_eq!(report.models, ["synthetic-sonnet"]);
    assert!(!report.model_catalog_fresh);
    let result = super::super::super::super::probe_result::from_harness_probe(&report, 1, 10, 1);
    assert_eq!(
        result.status,
        crate::harness::discovery::ProbeStatus::NeedsAuthentication
    );
    assert_eq!(
        result.failure_stage,
        Some(crate::harness::discovery::ProbeKind::Authentication)
    );
    assert_eq!(
        result.model_catalog_status,
        crate::harness::discovery::ModelCatalogStatus::Stale
    );

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);
    let detected = &settings.discovered["claude"];
    assert!(!detected.ready);
    assert_eq!(detected.models, ["synthetic-sonnet"]);
    assert!(!detected.model_catalog_fresh);
    assert_eq!(
        detected
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        ReadinessState::Unavailable
    );
    assert!(settings.default_harness.is_none());
    std::fs::remove_dir_all(root).unwrap();
}

/// Production boundary: configured model text crosses the CLI adapter into persisted provider
/// state. A parser-only test cannot prove it is filtered before persistence. Uses one fake Unix
/// CLI and temporary settings file; setup cost is one short probe.
#[cfg(unix)]
#[test]
fn claude_configured_model_output_cannot_persist_secret_text() {
    let root = std::env::temp_dir().join(format!(
        "koolade-claude-model-secret-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(root.join(".claude")).unwrap();
    std::fs::write(
        root.join(".claude/settings.json"),
        r#"{"model":"sonnet token=synthetic-secret"}"#,
    )
    .unwrap();
    let binary = root.join("claude");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--settings\" ]; then [ \"$2\" = '{\"env\":{\"DISABLE_UPDATES\":\"1\"}}' ] || exit 94; shift 2; fi\n[ \"$DISABLE_UPDATES\" = \"1\" ] || exit 93\nif [ \"$1\" = \"--version\" ]; then echo 'Claude Code v1.2.3'; exit 0; fi\nif [ \"$1\" = \"auth\" ]; then echo '{\"authMethod\":\"none\"}'; exit 0; fi\nexit 1\n",
    ).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::claude_harness::CLAUDE_BINARY_ENV,
        binary.as_os_str(),
    );
    environment.set("HOME", root.as_os_str());
    environment.set("KOOLADE_HOME", root.join("isolated-state").as_os_str());
    environment.remove(crate::harness::claude_harness::CLAUDE_MODEL_ENV);

    let report = discover_provider("claude", true);
    assert!(report.models.is_empty());
    assert!(!report.model_catalog_fresh);
    assert!(!report.status.contains("synthetic-secret"));
    assert!(
        !report
            .diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("synthetic-secret")
    );
    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);
    let detected = &settings.discovered["claude"];
    assert!(detected.models.is_empty());
    assert!(!detected.model_catalog_fresh);
    assert!(!detected.status.contains("synthetic-secret"));
    assert!(
        !detected
            .diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("synthetic-secret")
    );
    std::fs::remove_dir_all(root).unwrap();
}
