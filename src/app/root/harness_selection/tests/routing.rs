use super::*;
#[path = "routing/authentication.rs"]
pub(super) mod authentication;
#[path = "routing/configured_consumers.rs"]
mod configured_consumers;
#[path = "routing/model_catalog.rs"]
mod model_catalog;
#[path = "routing/model_provenance.rs"]
mod model_provenance;
#[path = "routing/role_categories.rs"]
mod role_categories;
#[test]
fn saved_supported_harness_ids_resolve_to_their_adapters() {
    assert!(resolve(Some("pi")).label().starts_with("pi "));
    assert!(resolve(Some("codex")).label().starts_with("codex "));
    assert!(resolve(Some("claude")).label().starts_with("claude "));
    assert!(resolve(Some("opencode")).label().starts_with("opencode"));
    assert!(
        resolve(Some("antigravity"))
            .label()
            .starts_with("Antigravity")
    );
}

#[test]
fn missing_user_default_does_not_silently_select_pi() {
    let harness = resolve(None);
    assert!(harness.label().contains("No CLI is selected"));
    assert!(
        harness
            .check_available()
            .unwrap_err()
            .detail()
            .contains("Choose a default in Coding tools")
    );
}

#[test]
fn work_categories_route_independently_and_unmapped_categories_use_the_global_default() {
    use crate::persistence::harness_settings::{HarnessSettings, WorkRoute};
    let settings = HarnessSettings {
        default_harness: Some("pi".into()),
        work_routes: std::collections::BTreeMap::from([
            (
                "implementation".into(),
                WorkRoute {
                    harness: "pi".into(),
                    model: Some("local-model".into()),
                },
            ),
            (
                "manager".into(),
                WorkRoute {
                    harness: "codex".into(),
                    model: Some("codex-model".into()),
                },
            ),
            (
                "qa_verification".into(),
                WorkRoute {
                    harness: "claude".into(),
                    model: Some("claude-model".into()),
                },
            ),
            (
                "documentation".into(),
                WorkRoute {
                    harness: "codex".into(),
                    model: None,
                },
            ),
        ]),
        ..HarnessSettings::default()
    };
    for (category, expected_harness, expected_model) in [
        ("implementation", "pi", Some("local-model")),
        ("manager", "codex", Some("codex-model")),
        ("qa_verification", "claude", Some("claude-model")),
        ("documentation", "codex", None),
    ] {
        let (harness, route) = route_selection(&settings, Some(category), None, None);
        assert_eq!(harness, Some(expected_harness));
        assert_eq!(
            route.and_then(|route| route.model.as_deref()),
            expected_model
        );
    }
    let (harness, route) = route_selection(&settings, Some("future-category"), None, None);
    assert_eq!(harness, Some("pi"));
    assert!(route.is_none());
}

#[test]
fn task_route_overrides_each_category_and_remain_stable_when_app_defaults_change() {
    use crate::persistence::harness_settings::{
        DOCUMENTATION, HarnessSettings, IMPLEMENTATION, QA, WorkRoute,
    };
    let routes: std::collections::BTreeMap<String, WorkRoute> = std::collections::BTreeMap::from([
        (
            IMPLEMENTATION.into(),
            WorkRoute {
                harness: "codex".into(),
                model: Some("task-model".into()),
            },
        ),
        (
            QA.into(),
            WorkRoute {
                harness: "claude".into(),
                model: None,
            },
        ),
    ]);
    let mut settings = HarnessSettings {
        default_harness: Some("pi".into()),
        work_routes: std::collections::BTreeMap::from([(
            IMPLEMENTATION.into(),
            WorkRoute {
                harness: "pi".into(),
                model: Some("app-model".into()),
            },
        )]),
        ..Default::default()
    };
    for (category, expected) in [
        (IMPLEMENTATION, "codex"),
        (QA, "claude"),
        (DOCUMENTATION, "pi"),
    ] {
        let (selected, route) =
            route_selection(&settings, Some(category), None, routes.get(category));
        assert_eq!(
            route.map(|route| route.harness.as_str()),
            (category != DOCUMENTATION).then_some(expected)
        );
        assert_eq!(selected, Some(expected));
    }
    settings.default_harness = Some("claude".into());
    settings.work_routes.insert(
        IMPLEMENTATION.into(),
        WorkRoute {
            harness: "claude".into(),
            model: Some("new-app-model".into()),
        },
    );
    let (_, route) = route_selection(
        &settings,
        Some(IMPLEMENTATION),
        None,
        routes.get(IMPLEMENTATION),
    );
    assert_eq!(
        route.map(|route| route.model.as_deref()),
        Some(Some("task-model"))
    );
}

#[test]
fn explicit_implementation_route_requires_provider_implementation_availability() {
    use crate::persistence::harness_settings::{
        DetectedHarness, HarnessSettings, IMPLEMENTATION, WorkRoute,
    };
    let detected = |id: &str, implementation_available| DetectedHarness {
        status: format!("{id} ready"),
        version: Some("1.0.0".into()),
        executable: Some(format!("/tools/{id}")),
        diagnostic: None,
        ready: true,
        models: vec![],
        default_model: None,
        model_catalog_fresh: false,
        configuration_required: false,
        implementation_available,
        checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        capability_snapshot: Some(authentication::ready_snapshot(id)),
    };
    let settings = HarnessSettings {
        default_harness: Some("pi".into()),
        discovered: std::collections::BTreeMap::from([
            ("pi".into(), detected("pi", true)),
            ("codex".into(), detected("codex", false)),
        ]),
        work_routes: std::collections::BTreeMap::from([(
            IMPLEMENTATION.into(),
            WorkRoute {
                harness: "codex".into(),
                model: None,
            },
        )]),
        ..HarnessSettings::default()
    };

    let harness = routed_harness(&settings, Some(IMPLEMENTATION), None, None);
    assert!(harness.label().to_ascii_lowercase().contains("codex"));
    let error = harness
        .check_available()
        .err()
        .map(|error| error.detail())
        .unwrap_or_default();

    assert!(error.contains("not currently available for implementation"));
}

#[test]
fn task_routes_reject_manager_unavailable_harness_and_stale_model() {
    use crate::persistence::harness_settings::{
        DetectedHarness, HarnessSettings, IMPLEMENTATION, MANAGER, WorkRoute,
    };
    let settings = HarnessSettings {
        discovered: std::collections::BTreeMap::from([(
            "codex".into(),
            DetectedHarness {
                status: "codex ready".into(),
                version: Some("1.0.0".into()),
                executable: Some("/synthetic/bin/codex".into()),
                diagnostic: None,
                ready: true,
                models: vec!["available".into()],
                default_model: None,
                model_catalog_fresh: true,
                configuration_required: false,
                implementation_available: false,
                checked_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
                capability_snapshot: Some(authentication::ready_snapshot("codex")),
            },
        )]),
        ..Default::default()
    };
    let route = |model: Option<&str>| WorkRoute {
        harness: "codex".into(),
        model: model.map(str::to_owned),
    };
    assert!(
        validate_task_routes(
            &settings,
            &std::collections::BTreeMap::from([(MANAGER.into(), route(None))])
        )
        .unwrap_err()
        .contains("application-routed")
    );
    assert!(
        validate_task_routes(
            &settings,
            &std::collections::BTreeMap::from([(IMPLEMENTATION.into(), route(Some("removed")))])
        )
        .unwrap_err()
        .contains("application-owned Docker execution session")
    );
    assert!(
        validate_task_routes(
            &settings,
            &std::collections::BTreeMap::from([(
                IMPLEMENTATION.into(),
                WorkRoute {
                    harness: "missing".into(),
                    model: None
                },
            )])
        )
        .unwrap_err()
        .contains("not been discovered")
    );
    let mut available = settings.clone();
    available
        .discovered
        .get_mut("codex")
        .unwrap()
        .implementation_available = true;
    assert!(
        validate_task_routes(
            &available,
            &std::collections::BTreeMap::from([(IMPLEMENTATION.into(), route(Some("removed")))])
        )
        .unwrap_err()
        .contains("Model 'removed' is unavailable")
    );
    validate_task_routes(
        &available,
        &std::collections::BTreeMap::from([(IMPLEMENTATION.into(), route(Some("available")))]),
    )
    .unwrap();
    let mut context_only = settings.clone();
    let capabilities = context_only
        .discovered
        .get_mut("codex")
        .unwrap()
        .capability_snapshot
        .as_mut()
        .unwrap();
    capabilities.docker_mcp_bridge = crate::execution::contract::ReadinessState::Unavailable;
    capabilities.execution_target = crate::execution::contract::ReadinessState::Unavailable;
    validate_task_routes(
        &context_only,
        &std::collections::BTreeMap::from([(
            crate::persistence::harness_settings::DOCUMENTATION.into(),
            route(Some("available")),
        )]),
    )
    .expect("context-only documentation does not require an implementation sandbox");
}
