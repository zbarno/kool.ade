use super::*;

#[test]
fn target_default_loads_automatic_and_round_trips_local_preference() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    assert_eq!(load_from(&path).0.default_target, TargetPreference::Auto);
    let settings = ExecutionEnvironmentSettings {
        schema_version: 1,
        default_target: TargetPreference::Local,
        task_targets: Default::default(),
        task_capabilities: Default::default(),
        task_allowed_targets: Default::default(),
    };
    save_to(&path, &settings).unwrap();
    assert_eq!(load_from(&path), (settings, None));
}

#[test]
fn target_settings_reject_untrusted_worker_identifiers() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    let settings = ExecutionEnvironmentSettings {
        schema_version: 1,
        default_target: TargetPreference::Worker("../other-worker".into()),
        task_targets: Default::default(),
        task_capabilities: Default::default(),
        task_allowed_targets: Default::default(),
    };
    assert!(save_to(&path, &settings).is_err());
    assert!(!path.exists());
}

#[test]
fn task_target_preferences_round_trip_and_reject_unsafe_keys() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    let mut settings = ExecutionEnvironmentSettings::default();
    settings
        .task_targets
        .insert("tasks/feature-123.md".into(), TargetPreference::Local);
    save_to(&path, &settings).unwrap();
    assert_eq!(
        load_from(&path).0.task_targets.get("tasks/feature-123.md"),
        Some(&TargetPreference::Local)
    );

    settings
        .task_targets
        .insert("../outside.md".into(), TargetPreference::Auto);
    assert!(save_to(&path, &settings).is_err());
}

#[test]
fn old_settings_without_task_targets_keep_their_default() {
    let settings: ExecutionEnvironmentSettings =
        serde_json::from_str(r#"{"schemaVersion":1,"defaultTarget":{"kind":"auto"}}"#).unwrap();
    assert!(settings.task_targets.is_empty());
    assert!(settings.task_capabilities.is_empty());
    assert!(settings.task_allowed_targets.is_empty());
}

#[test]
fn task_target_allowlists_round_trip_and_reject_invalid_members() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    let targets = vec![
        TargetPreference::Local,
        TargetPreference::Worker("worker-east".into()),
    ];
    let mut settings = ExecutionEnvironmentSettings::default();
    settings
        .task_allowed_targets
        .insert("task-123".into(), targets.clone());
    save_to(&path, &settings).unwrap();
    assert_eq!(load_from(&path).0.task_allowed_targets["task-123"], targets);

    settings
        .task_allowed_targets
        .insert("task-123".into(), vec![TargetPreference::Auto]);
    assert!(save_to(&path, &settings).is_err());
    settings.task_allowed_targets.insert(
        "task-123".into(),
        vec![TargetPreference::Local, TargetPreference::Local],
    );
    assert!(save_to(&path, &settings).is_err());
}

#[test]
fn task_target_allowlist_update_and_clear_semantics() {
    let mut settings = ExecutionEnvironmentSettings::default();
    update_task_allowed_targets(&mut settings, "task-123", vec![TargetPreference::Local]);
    assert_eq!(
        settings.task_allowed_targets["task-123"],
        [TargetPreference::Local]
    );

    update_task_allowed_targets(&mut settings, "task-123", Vec::new());
    assert!(!settings.task_allowed_targets.contains_key("task-123"));
}

#[test]
fn loading_semantically_invalid_settings_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    std::fs::write(
        &path,
        r#"{"schemaVersion":1,"defaultTarget":{"kind":"auto"},"taskAllowedTargets":{"task-123":[{"kind":"auto"}]}}"#,
    )
    .unwrap();

    let (settings, error) = load_from(&path);
    assert!(error.is_some());
    assert_eq!(settings, ExecutionEnvironmentSettings::default());
}

#[test]
fn task_capability_requirements_are_operator_local_and_round_trip() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    let mut settings = ExecutionEnvironmentSettings::default();
    settings.task_capabilities.insert(
        "task-123".into(),
        TaskCapabilityRequirements {
            requires_gpu: true,
            requires_display: false,
        },
    );
    save_to(&path, &settings).unwrap();
    let restored = load_from(&path).0;
    assert_eq!(restored.task_capabilities, settings.task_capabilities);
    assert!(
        save_to(
            &path,
            &ExecutionEnvironmentSettings {
                task_capabilities: [("../outside".into(), TaskCapabilityRequirements::default())]
                    .into(),
                ..ExecutionEnvironmentSettings::default()
            }
        )
        .is_err()
    );
}
