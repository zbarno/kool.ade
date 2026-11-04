use std::fs;

#[test]
fn setup_store_uses_only_the_current_application_data_root() {
    let temp = tempfile::tempdir().unwrap();
    let old_path = temp.path().join(".koolade-packet/machine-setup/tasks.json");
    fs::create_dir_all(old_path.parent().unwrap()).unwrap();
    fs::write(&old_path, b"old private setup state").unwrap();
    let app_data_root = temp.path().join("platform-data");

    let current_path = super::path_for_app_data_root(&app_data_root);
    assert_eq!(current_path, app_data_root.join("machine-setup/tasks.json"));
    assert!(!current_path.exists());
    assert_eq!(fs::read(&old_path).unwrap(), b"old private setup state");
    assert!(
        !app_data_root
            .join(".machine-setup-migration-staging")
            .exists()
    );
}
