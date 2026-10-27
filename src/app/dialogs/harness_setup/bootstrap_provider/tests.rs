use super::*;

#[test]
fn guided_setup_choice_does_not_change_default_or_manager_route() {
    let manager_route = crate::persistence::harness_settings::WorkRoute {
        harness: "pi".into(),
        model: Some("manager-model".into()),
    };
    let mut settings = HarnessSettings {
        default_harness: Some("pi".into()),
        work_routes: std::collections::BTreeMap::from([(
            crate::persistence::harness_settings::MANAGER.into(),
            manager_route.clone(),
        )]),
        ..HarnessSettings::default()
    };

    assert!(set_provider(&mut settings, "codex"));

    assert_eq!(settings.bootstrap_provider.as_deref(), Some("codex"));
    assert_eq!(settings.default_harness.as_deref(), Some("pi"));
    assert_eq!(
        settings.work_routes[crate::persistence::harness_settings::MANAGER],
        manager_route
    );
}

#[test]
fn unsupported_guided_provider_does_not_mutate_settings() {
    let mut settings = HarnessSettings::default();
    assert!(!set_provider(&mut settings, "unknown"));
    assert_eq!(settings.bootstrap_provider, None);
}
