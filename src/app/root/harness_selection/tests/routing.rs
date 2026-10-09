use super::*;
#[path = "routing/model_catalog.rs"]
mod model_catalog;
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
fn unsupported_implementation_route_fails_closed_without_falling_back_to_pi() {
    use crate::persistence::harness_settings::{
        DetectedHarness, HarnessSettings, IMPLEMENTATION, WorkRoute,
    };
    let detected = |id: &str, implementation_available| DetectedHarness {
        status: format!("{id} ready"),
        version: None,
        executable: Some(format!("/tools/{id}")),
        diagnostic: None,
        ready: true,
        models: vec![],
        default_model: None,
        configuration_required: false,
        implementation_available,
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

    let error = routed_harness(&settings, Some(IMPLEMENTATION), None, None)
        .check_available()
        .unwrap_err()
        .detail();

    assert!(error.contains("codex"));
    assert!(error.contains("application-owned Linux sandbox"));
    assert!(error.contains("no fallback"));
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
                version: None,
                executable: None,
                diagnostic: None,
                ready: true,
                models: vec!["available".into()],
                default_model: None,
                configuration_required: false,
                implementation_available: false,
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
        .contains("application-owned Linux sandbox")
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
    assert!(
        validate_task_routes(
            &settings,
            &std::collections::BTreeMap::from([(
                crate::persistence::harness_settings::DOCUMENTATION.into(),
                route(Some("available")),
            )])
        )
        .unwrap_err()
        .contains("application-owned Linux sandbox")
    );
}
