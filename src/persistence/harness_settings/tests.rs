use super::*;

fn temp_file() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "koolade-harness-settings-{}.json",
        uuid::Uuid::new_v4()
    ))
}

#[test]
fn settings_round_trip_persists_default_and_discovery_diagnostics() {
    let path = temp_file();
    let settings = HarnessSettings {
        schema_version: 0,
        default_harness: Some("pi".into()),
        work_routes: BTreeMap::from([(
            "implementation".into(),
            WorkRoute {
                harness: "codex".into(),
                model: Some("gpt-6-sol".into()),
            },
        )]),
        discovered: BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                status: "pi 0.30.1".into(),
                version: Some("0.30.1".into()),
                executable: Some("/example/bin/pi".into()),
                diagnostic: None,
                ready: true,
                models: vec!["qwen-local".into()],
                default_model: None,
                configuration_required: false,
                implementation_available: true,
            },
        )]),
        manual_executable_paths: BTreeMap::from([("pi".into(), "/example/bin/pi custom".into())]),
    };
    save_to(&path, &settings).unwrap();
    let loaded: HarnessSettings = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(loaded.schema_version, 2);
    assert_eq!(loaded.default_harness.as_deref(), Some("pi"));
    assert_eq!(loaded.work_routes["implementation"].harness, "codex");
    assert_eq!(
        loaded.work_routes["implementation"].model.as_deref(),
        Some("gpt-6-sol")
    );
    assert!(loaded.discovered["pi"].ready);
    assert_eq!(
        loaded.manual_executable_paths["pi"],
        "/example/bin/pi custom"
    );
    std::fs::remove_file(path).unwrap();
}

#[test]
fn unavailable_tools_remain_recorded_without_rewriting_the_default() {
    let path = temp_file();
    let settings = HarnessSettings {
        schema_version: 1,
        default_harness: Some("pi".into()),
        work_routes: BTreeMap::new(),
        discovered: BTreeMap::from([(
            "pi".into(),
            DetectedHarness {
                status: "pi (not installed)".into(),
                version: None,
                executable: None,
                diagnostic: Some("not found".into()),
                ready: false,
                models: Vec::new(),
                default_model: None,
                configuration_required: false,
                implementation_available: false,
            },
        )]),
        manual_executable_paths: BTreeMap::new(),
    };
    save_to(&path, &settings).unwrap();
    let loaded: HarnessSettings = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(loaded.default_harness.as_deref(), Some("pi"));
    assert!(!loaded.discovered["pi"].ready);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn version_one_settings_migrate_without_losing_default_or_discovery() {
    let path = temp_file();
    std::fs::write(
        &path,
        r#"{"schemaVersion":1,"defaultHarness":"pi","discovered":{}}"#,
    )
    .unwrap();
    let (settings, diagnostic) = load_from(&path);
    assert!(diagnostic.is_none());
    assert_eq!(settings.schema_version, 2);
    assert_eq!(settings.default_harness.as_deref(), Some("pi"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn unknown_settings_versions_are_reported_and_not_interpreted() {
    let path = temp_file();
    std::fs::write(&path, r#"{"schemaVersion":9,"defaultHarness":"pi"}"#).unwrap();
    let (settings, diagnostic) = load_from(&path);
    assert!(settings.default_harness.is_none());
    assert!(
        diagnostic
            .unwrap()
            .contains("Unsupported harness settings version 9")
    );
    std::fs::remove_file(path).unwrap();
}
