use super::*;

#[test]
fn explicit_implementation_route_rejects_unsupported_provider_capability() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let settings = settings_with("unknown", ReadinessState::Ready, now);
    let routes = std::collections::BTreeMap::from([(
        IMPLEMENTATION.into(),
        WorkRoute {
            harness: "unknown".into(),
            model: None,
        },
    )]);
    let path = std::env::temp_dir().join(format!(
        "harness-route-capability-{}.json",
        uuid::Uuid::new_v4()
    ));

    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let mut override_harness = None;
        let harness =
            configured_harness_for_task(&mut override_harness, Some(IMPLEMENTATION), &routes);
        let error = harness.check_available().unwrap_err().detail();
        assert!(error.contains("no supported application-owned implementation route"));
    });
    let _ = std::fs::remove_file(path);
}

#[test]
fn uncategorized_implementation_route_rejects_runtime_unavailable_provider() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let mut settings = settings_with("codex", ReadinessState::Ready, now);
    settings
        .discovered
        .get_mut("codex")
        .unwrap()
        .implementation_available = false;
    let routes = std::collections::BTreeMap::from([(
        IMPLEMENTATION.into(),
        WorkRoute {
            harness: "codex".into(),
            model: None,
        },
    )]);
    let path = std::env::temp_dir().join(format!(
        "harness-route-runtime-unavailable-{}.json",
        uuid::Uuid::new_v4()
    ));

    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let mut override_harness = None;
        let harness =
            configured_harness_for_task(&mut override_harness, Some(IMPLEMENTATION), &routes);
        let error = harness.check_available().unwrap_err().detail();
        assert!(error.contains("not currently available for implementation"));
    });
    let _ = std::fs::remove_file(path);
}

#[test]
fn context_only_default_route_does_not_require_implementation_runtime() {
    let now = crate::persistence::harness_settings::current_unix_ms();
    let mut settings = settings_with("codex", ReadinessState::Ready, now);
    settings
        .discovered
        .get_mut("codex")
        .unwrap()
        .implementation_available = false;
    let path = std::env::temp_dir().join(format!(
        "harness-context-only-{}.json",
        uuid::Uuid::new_v4()
    ));

    crate::persistence::harness_settings::with_settings_path(path.clone(), || {
        crate::persistence::harness_settings::save(&settings).unwrap();
        let (selected, route) = route_selection(&settings, None, None, None);
        assert_eq!(selected, Some("codex"));
        assert!(route.is_none());
    });
    let _ = std::fs::remove_file(path);
}
