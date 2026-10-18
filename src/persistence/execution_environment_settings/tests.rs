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
