//! Cross the real OpenCode CLI adapter, settings persistence, and task-routing boundary. The
//! invariant is that a documented authenticated-provider summary yields a current selectable
//! route; parser tests cannot prove adapter persistence or routing. Uses one synthetic Unix CLI
//! and temporary config; setup cost is one short probe.

use super::*;
use std::os::unix::fs::PermissionsExt;

#[test]
fn authenticated_provider_summary_persists_and_authorizes_the_implementation_route() {
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-authenticated-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("opencode");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'opencode 1.2.3'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"models\" ]; then echo 'synthetic/openai-model'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"auth\" ] && [ \"$3\" = \"list\" ]; then printf '%s\\n' '┌ Credentials /synthetic/auth.json' '● Synthetic API' '└ 1 credential' '┌ Environment' '└ 0 environment variables'; exit 0; fi\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::opencode_harness::OPENCODE_BINARY_ENV,
        binary.as_os_str(),
    );

    let report = discover_provider("opencode", true);
    assert!(report.ready);
    assert!(report.implementation_available);
    assert!(report.model_catalog_fresh);
    assert_eq!(
        report.authentication,
        crate::execution::contract::ReadinessState::Ready
    );
    assert!(
        !report
            .diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("Synthetic API")
    );

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report],
        Some("opencode"),
        1,
    );
    assert_eq!(settings.default_harness.as_deref(), Some("opencode"));
    let detected = &settings.discovered["opencode"];
    assert!(detected.ready);
    assert!(detected.implementation_available);
    assert!(
        super::super::super::super::has_current_provider_capabilities(
            "opencode",
            detected,
            crate::persistence::harness_settings::current_unix_ms()
        )
    );
    assert_eq!(
        detected
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        crate::execution::contract::ReadinessState::Ready
    );
    let advertised_route = std::collections::BTreeMap::from([(
        crate::persistence::harness_settings::IMPLEMENTATION.to_owned(),
        crate::persistence::harness_settings::WorkRoute {
            harness: "opencode".into(),
            model: Some("synthetic/openai-model".into()),
        },
    )]);
    assert!(
        crate::app::root::harness_selection::validate_task_routes(&settings, &advertised_route)
            .is_ok()
    );
    let unadvertised_route = std::collections::BTreeMap::from([(
        crate::persistence::harness_settings::IMPLEMENTATION.to_owned(),
        crate::persistence::harness_settings::WorkRoute {
            harness: "opencode".into(),
            model: Some("ollama/llama3.2".into()),
        },
    )]);
    assert!(
        crate::app::root::harness_selection::validate_task_routes(&settings, &unadvertised_route)
            .is_err()
    );

    std::fs::remove_dir_all(root).unwrap();
}

/// Production boundary: a failed auth-list command is translated and persisted as unknown
/// authentication. A parser test cannot prove routing remains blocked; one fake Unix CLI and a
/// temporary settings file are used, with low setup cost.
#[test]
fn explicit_auth_list_failure_is_persisted_as_unknown() {
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-auth-failure-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let binary = root.join("opencode");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'opencode 1.2.3'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"models\" ]; then echo 'synthetic/openai-model'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"auth\" ]; then echo 'Unauthorized synthetic failure' >&2; exit 1; fi\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::opencode_harness::OPENCODE_BINARY_ENV,
        binary.as_os_str(),
    );

    let report = discover_provider("opencode", true);
    assert_eq!(
        report.authentication,
        crate::execution::contract::ReadinessState::Unknown
    );
    assert!(
        !report
            .diagnostic
            .as_deref()
            .unwrap_or_default()
            .contains("Unauthorized")
    );
    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report],
        Some("opencode"),
        1,
    );
    assert!(settings.default_harness.is_none());
    assert!(!settings.discovered["opencode"].ready);
    assert_eq!(
        settings.discovered["opencode"]
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        crate::execution::contract::ReadinessState::Unknown
    );

    std::fs::remove_dir_all(root).unwrap();
}

/// Production boundary: an empty credential listing must still permit a documented local
/// model, while remote model IDs from the same catalog remain unselectable. Parser tests alone
/// cannot prove the adapter filters persisted models and produces a usable capability snapshot.
#[test]
fn local_model_without_credentials_remains_selectable_and_remote_models_are_filtered() {
    let root = std::env::temp_dir().join(format!(
        "koolade-opencode-local-model-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(root.join(".config/opencode")).unwrap();
    std::fs::write(
        root.join(".config/opencode/opencode.json"),
        r#"{"provider":{"ollama":{"options":{"baseURL":"http://127.0.0.1:11434/v1"}}}}"#,
    )
    .unwrap();
    let binary = root.join("opencode");
    std::fs::write(
        &binary,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'opencode 1.2.3'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"models\" ]; then printf '%s\\n' 'ollama/llama3.2' 'openai/gpt token=synthetic-secret' 'openai/gpt-sig?X-Amz-Signature=synthetic-secret'; exit 0; fi\nif [ \"$1\" = \"--pure\" ] && [ \"$2\" = \"auth\" ] && [ \"$3\" = \"list\" ]; then printf '%s\\n' '┌ Credentials /synthetic/auth.json' '└ 0 credentials' '┌ Environment' '└ 0 environment variables'; exit 0; fi\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set(
        crate::harness::opencode_harness::OPENCODE_BINARY_ENV,
        binary.as_os_str(),
    );
    environment.set("HOME", root.as_os_str());
    environment.remove("XDG_CONFIG_HOME");
    environment.remove("OPENCODE_CONFIG");
    environment.remove("OPENCODE_CONFIG_CONTENT");

    let report = discover_provider("opencode", true);
    assert!(report.ready);
    assert!(report.implementation_available);
    assert_eq!(report.models, ["ollama/llama3.2"]);
    assert!(report.model_catalog_fresh);
    assert!(
        !report
            .models
            .iter()
            .any(|model| model.contains("synthetic-secret"))
    );
    assert_eq!(
        report.authentication,
        crate::execution::contract::ReadinessState::Ready
    );

    let mut settings = crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(
        &mut settings,
        &[report],
        Some("opencode"),
        1,
    );
    let detected = &settings.discovered["opencode"];
    assert_eq!(settings.default_harness.as_deref(), Some("opencode"));
    assert!(detected.ready);
    assert!(detected.implementation_available);
    assert_eq!(detected.models, ["ollama/llama3.2"]);
    assert!(
        !detected
            .models
            .iter()
            .any(|model| model.contains("synthetic-secret"))
    );
    assert!(
        super::super::super::super::has_current_provider_capabilities(
            "opencode",
            detected,
            crate::persistence::harness_settings::current_unix_ms()
        )
    );
    assert_eq!(
        detected
            .capability_snapshot
            .as_ref()
            .unwrap()
            .authentication,
        crate::execution::contract::ReadinessState::Ready
    );
    let local_route = std::collections::BTreeMap::from([(
        crate::persistence::harness_settings::IMPLEMENTATION.to_owned(),
        crate::persistence::harness_settings::WorkRoute {
            harness: "opencode".into(),
            model: Some("ollama/llama3.2".into()),
        },
    )]);
    assert!(
        crate::app::root::harness_selection::validate_task_routes(&settings, &local_route).is_ok()
    );
    let remote_route = std::collections::BTreeMap::from([(
        crate::persistence::harness_settings::IMPLEMENTATION.to_owned(),
        crate::persistence::harness_settings::WorkRoute {
            harness: "opencode".into(),
            model: Some("openai/gpt-synthetic".into()),
        },
    )]);
    assert!(
        crate::app::root::harness_selection::validate_task_routes(&settings, &remote_route)
            .is_err()
    );

    std::fs::write(
        root.join(".config/opencode/opencode.json"),
        r#"{"provider":{"ollama":{"options":{"baseURL":"https://remote.example.test/v1"}}}}"#,
    )
    .unwrap();
    let remote_endpoint_report = discover_provider("opencode", true);
    assert_eq!(
        remote_endpoint_report.authentication,
        crate::execution::contract::ReadinessState::Unavailable
    );
    let mut remote_endpoint_settings =
        crate::persistence::harness_settings::HarnessSettings::default();
    super::super::super::super::discovery::apply_probe_results(
        &mut remote_endpoint_settings,
        &[remote_endpoint_report],
        None,
        2,
    );
    assert!(
        crate::app::root::harness_selection::validate_task_routes(
            &remote_endpoint_settings,
            &local_route
        )
        .is_err()
    );

    std::fs::remove_dir_all(root).unwrap();
}
