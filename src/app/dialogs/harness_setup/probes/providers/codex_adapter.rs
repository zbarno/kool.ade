use super::super::{ReadinessState, discover_provider};
use std::os::unix::fs::PermissionsExt;

/// Covers the production adapter, typed result conversion, and saved settings boundary. The fake
/// CLI proves an auth failure remains distinct from a successful model catalog; parser tests alone
/// cannot verify that mapping. Uses a Unix shell and temporary files; setup cost is one short CLI.
#[cfg(unix)]
#[test]
fn codex_auth_failure_keeps_catalog_evidence_and_blocks_selection() {
    let root = std::env::temp_dir().join(format!("koolade-codex-adapter-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    std::fs::write(
        &binary,
        r##"#!/bin/sh
if [ "$1" = "--version" ]; then echo 'codex-cli 1.2.3 token=synthetic-secret'; exit 0; fi
if [ "$1" = "exec" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'; exit 0; fi
if [ "$1" = "login" ]; then echo 'Not logged in'; exit 1; fi
if [ "$1" = "app-server" ]; then
  while read -r message; do
    case "$message" in
      *'"id":1'*) echo '{"id":1,"result":{}}' ;;
      *'"method":"model/list"'*) echo '{"id":2,"result":{"data":[{"model":"gpt-synthetic","isDefault":true}]}}'; exit 0 ;;
    esac
  done
fi
exit 1
"##,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::codex_harness::CODEX_BINARY_ENV,
        binary.as_os_str(),
    );
    environment.set("HOME", root.as_os_str());
    environment.set("KOOLADE_HOME", root.join("isolated-state").as_os_str());
    environment.remove(crate::harness::codex_harness::CODEX_MODEL_ENV);

    let report = discover_provider("codex", true);
    assert!(!report.ready);
    assert_eq!(report.authentication, ReadinessState::Unavailable);
    assert!(report.configuration_required);
    assert_eq!(report.models, ["gpt-synthetic"]);
    assert!(report.model_catalog_fresh);
    assert_eq!(report.version.as_deref(), Some("1.2.3"));
    assert!(!report.status.contains("synthetic-secret"));
    assert!(
        !report
            .diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("synthetic-secret")
    );
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
        crate::harness::discovery::ModelCatalogStatus::Fresh
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
    super::super::super::super::discovery::apply_probe_results(&mut settings, &[report], None, 1);
    let detected = &settings.discovered["codex"];
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
    assert_eq!(detected.models, ["gpt-synthetic"]);
    assert!(detected.model_catalog_fresh);
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

/// Production boundary: fake app-server output crosses discovery into saved provider state. A
/// parser-only check cannot prove secret-like model text is rejected before persistence. Uses a
/// Unix shell and temporary files; setup cost is one short app-server exchange.
#[cfg(unix)]
#[test]
fn codex_model_output_with_secret_text_is_rejected_before_persistence() {
    let root = std::env::temp_dir().join(format!(
        "koolade-codex-model-secret-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("codex");
    std::fs::write(
        &binary,
        r##"#!/bin/sh
if [ "$1" = "--version" ]; then echo 'codex-cli 1.2.3'; exit 0; fi
if [ "$1" = "exec" ]; then echo '--json --cd --sandbox --ephemeral --model --config --disable --ignore-user-config --skip-git-repo-check'; exit 0; fi
if [ "$1" = "login" ]; then echo 'Logged in'; exit 0; fi
if [ "$1" = "app-server" ]; then
  while read -r message; do
    case "$message" in
      *'"id":1'*) echo '{"id":1,"result":{}}' ;;
      *'"method":"model/list"'*) echo '{"id":2,"result":{"data":[{"model":"gpt-5 token=synthetic-secret","isDefault":true}]}}'; exit 0 ;;
    esac
  done
fi
exit 1
"##,
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::codex_harness::CODEX_BINARY_ENV,
        binary.as_os_str(),
    );
    environment.set("HOME", root.as_os_str());
    environment.set("KOOLADE_HOME", root.join("isolated-state").as_os_str());

    let report = discover_provider("codex", true);
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
    let detected = &settings.discovered["codex"];
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
