use std::fs;

use super::{migration, root::Roots};

fn test_roots(root: &std::path::Path, legacy: &std::path::Path) -> Roots {
    Roots {
        app_data_root: root.join("platform-data"),
        legacy_state_root: Some(legacy.to_path_buf()),
    }
}

#[test]
fn migration_copies_setup_state_verifies_it_and_keeps_the_legacy_source() {
    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join(".koolade-packet");
    let source = legacy.join("machine-setup");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("tasks.json"), "saved setup task state").unwrap();
    let roots = test_roots(temp.path(), &legacy);

    let path = migration::resolve(&roots).unwrap();

    assert_eq!(
        path,
        temp.path().join("platform-data/machine-setup/tasks.json")
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "saved setup task state");
    assert_eq!(
        fs::read_to_string(source.join("tasks.json")).unwrap(),
        "saved setup task state"
    );
    assert!(
        path.parent()
            .unwrap()
            .join(".koolade-machine-setup-migration-v1")
            .is_file()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn repair_audit_writes_do_not_change_the_migrated_legacy_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join(".koolade-packet");
    let source = legacy.join("machine-setup");
    fs::create_dir_all(&source).unwrap();
    let initial = crate::persistence::setup_tasks::SetupTaskStoreV1::default();
    fs::write(
        source.join("tasks.json"),
        serde_json::to_vec(&initial).unwrap(),
    )
    .unwrap();
    let roots = test_roots(temp.path(), &legacy);

    let state_directory = super::state_dir_for_roots(&roots).unwrap();
    let audit_path = crate::app::bootstrap_approval::repair_audit_path(&state_directory);
    crate::app::bootstrap_approval::append_audit_for_test(&audit_path).unwrap();

    let tasks_path = migration::resolve(&roots).unwrap();
    let (restored, _) = crate::persistence::setup_tasks::load_from_for_test(&tasks_path).unwrap();

    assert_eq!(restored, initial);
    assert!(audit_path.is_file());
    assert_eq!(
        serde_json::from_slice::<crate::persistence::setup_tasks::SetupTaskStoreV1>(
            &fs::read(source.join("tasks.json")).unwrap()
        )
        .unwrap(),
        initial
    );
}

#[test]
fn migration_retains_a_shared_legacy_lock_and_blocks_older_writers() {
    use fs2::FileExt;
    use std::fs::OpenOptions;

    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join(".koolade-packet");
    let source = legacy.join("machine-setup");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("tasks.json"), "saved setup task state").unwrap();
    let roots = test_roots(temp.path(), &legacy);
    migration::resolve(&roots).unwrap();
    let lock_path = source.join("tasks.lock");

    let reader = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .unwrap();
    FileExt::try_lock_shared(&reader).unwrap();
    FileExt::unlock(&reader).unwrap();

    let older_writer = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .unwrap();
    assert!(FileExt::try_lock_exclusive(&older_writer).is_err());
}

#[test]
fn migration_is_idempotent_and_ignores_legacy_lock_and_instance_leases() {
    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join(".koolade-packet");
    let source = legacy.join("machine-setup");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("tasks.json"), "first snapshot").unwrap();
    let roots = test_roots(temp.path(), &legacy);

    let first = migration::resolve(&roots).unwrap();
    fs::write(source.join("tasks.lock"), "transient lock contents").unwrap();
    fs::create_dir_all(source.join("instances")).unwrap();
    fs::write(source.join("instances/stale-owner.lock"), "stale lease").unwrap();
    let second = migration::resolve(&roots).unwrap();

    assert_eq!(first, second);
    assert_eq!(fs::read_to_string(second).unwrap(), "first snapshot");
    assert_eq!(
        fs::read_to_string(source.join("tasks.json")).unwrap(),
        "first snapshot"
    );
}

#[test]
fn legacy_store_divergence_fails_closed_and_preserves_both_copies() {
    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join(".koolade-packet");
    let source = legacy.join("machine-setup");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("tasks.json"), "first snapshot").unwrap();
    let roots = test_roots(temp.path(), &legacy);
    let platform_path = migration::resolve(&roots).unwrap();

    fs::write(source.join("tasks.json"), "legacy version later wrote").unwrap();
    let error = migration::resolve(&roots).unwrap_err().to_string();

    assert!(error.contains("legacy machine setup state changed after migration"));
    assert_eq!(fs::read_to_string(platform_path).unwrap(), "first snapshot");
    assert_eq!(
        fs::read_to_string(source.join("tasks.json")).unwrap(),
        "legacy version later wrote"
    );
}

#[test]
fn migration_fails_closed_when_both_roots_have_unreceipted_data() {
    let temp = tempfile::tempdir().unwrap();
    let roots = test_roots(temp.path(), &temp.path().join(".koolade-packet"));
    let destination = roots.app_data_root.join("machine-setup");
    let legacy = roots
        .legacy_state_root
        .as_ref()
        .unwrap()
        .join("machine-setup");
    fs::create_dir_all(&destination).unwrap();
    fs::create_dir_all(&legacy).unwrap();
    fs::write(destination.join("tasks.json"), "platform state").unwrap();
    fs::write(legacy.join("tasks.json"), "legacy state").unwrap();

    let error = migration::resolve(&roots).unwrap_err().to_string();

    assert!(error.contains("both exist without a migration receipt"));
    assert_eq!(
        fs::read_to_string(destination.join("tasks.json")).unwrap(),
        "platform state"
    );
    assert_eq!(
        fs::read_to_string(legacy.join("tasks.json")).unwrap(),
        "legacy state"
    );
}

#[test]
fn interrupted_staging_is_discarded_and_recopied_from_the_preserved_source() {
    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join(".koolade-packet");
    let source = legacy.join("machine-setup");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("tasks.json"), "complete source").unwrap();
    let roots = test_roots(temp.path(), &legacy);
    fs::create_dir_all(roots.app_data_root.join(".machine-setup-migration-staging")).unwrap();
    fs::write(
        roots
            .app_data_root
            .join(".machine-setup-migration-staging/partial"),
        "interrupted copy",
    )
    .unwrap();

    let path = migration::resolve(&roots).unwrap();

    assert_eq!(fs::read_to_string(path).unwrap(), "complete source");
}

#[cfg(unix)]
#[test]
fn migration_rejects_symlinks_without_following_them() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join(".koolade-packet");
    let source = legacy.join("machine-setup");
    fs::create_dir_all(&source).unwrap();
    let outside = temp.path().join("outside.json");
    fs::write(&outside, "outside").unwrap();
    symlink(&outside, source.join("tasks.json")).unwrap();
    let roots = test_roots(temp.path(), &legacy);

    let error = migration::resolve(&roots).unwrap_err().to_string();

    assert!(error.contains("symlink or reparse point"));
    assert_eq!(fs::read_to_string(outside).unwrap(), "outside");
    assert!(!roots.app_data_root.join("machine-setup").exists());
}

#[test]
fn an_empty_platform_store_without_legacy_state_gets_a_fresh_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join(".koolade-packet");
    let roots = test_roots(temp.path(), &legacy);

    let first = migration::resolve(&roots).unwrap();
    let second = migration::resolve(&roots).unwrap();

    assert_eq!(first, second);
    assert!(
        first
            .parent()
            .unwrap()
            .join(".koolade-machine-setup-migration-v1")
            .is_file()
    );
}

#[test]
fn legacy_state_appearing_after_a_fresh_receipt_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let legacy = temp.path().join(".koolade-packet");
    let roots = test_roots(temp.path(), &legacy);
    let platform_path = migration::resolve(&roots).unwrap();
    let legacy_state = legacy.join("machine-setup/tasks.json");
    fs::write(&legacy_state, "state written by older install").unwrap();

    let error = migration::resolve(&roots).unwrap_err().to_string();

    assert!(
        error.contains("legacy machine setup state appeared after platform state was initialized")
    );
    assert!(!platform_path.exists());
    assert_eq!(
        fs::read_to_string(legacy_state).unwrap(),
        "state written by older install"
    );
    assert!(
        platform_path
            .parent()
            .unwrap()
            .join(".koolade-machine-setup-migration-v1")
            .is_file()
    );
}
