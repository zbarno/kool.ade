use super::*;

#[test]
fn fresh_install_is_off_and_unconfirmed_mode_cannot_be_saved() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    assert_eq!(load_from(&path).0, ExperimentalFeatureSettingsV1::default());
    let settings = ExperimentalFeatureSettingsV1 {
        enabled: true,
        ..ExperimentalFeatureSettingsV1::default()
    };
    assert!(save_to(&path, &settings).is_err());
    assert!(!path.exists());
}

#[test]
fn confirmed_opt_in_is_private_and_feature_toggles_stay_off_with_mode() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    let settings = ExperimentalFeatureSettingsV1 {
        enabled: true,
        confirmed_at_unix_ms: Some(1),
        enabled_features: [ExperimentalFeature::BrowserQa].into(),
        ..ExperimentalFeatureSettingsV1::default()
    };
    save_to(&path, &settings).unwrap();
    assert_eq!(load_from(&path), (settings.clone(), None));
    assert!(settings.feature_enabled(ExperimentalFeature::BrowserQa));
    assert!(!settings.feature_enabled(ExperimentalFeature::LinuxDesktopQa));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn malformed_or_unconfirmed_persisted_opt_in_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join(FILE);
    std::fs::write(
        &path,
        r#"{"schemaVersion":1,"enabled":true,"enabledFeatures":["browser_qa"]}"#,
    )
    .unwrap();
    let (settings, error) = load_from(&path);
    assert!(error.is_some());
    assert_eq!(settings, ExperimentalFeatureSettingsV1::default());
}
