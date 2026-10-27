use super::*;

#[path = "synchronization/recipes.rs"]
mod recipes;

#[cfg(unix)]
#[test]
fn synchronization_preserves_host_files() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    fs::write(fixture.0.join("source.txt"), b"host edit").unwrap();
    let archive = returned(&[("source.txt", b"container edit")]);
    assert!(synchronize(&workspace, &snapshot, &archive, 1024 * 1024).is_err());
    assert_eq!(
        fs::read(fixture.0.join("source.txt")).unwrap(),
        b"host edit"
    );
}

#[cfg(unix)]
#[test]
fn stage_out_rejects_a_replaced_workspace_root_identity() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned(&[("source.txt", b"container edit")]);
    let entries = super::super::archive::parse_returned(&archive, 1024 * 1024, 10).unwrap();
    let moved = fixture.0.with_extension("moved");
    fs::rename(&fixture.0, &moved).unwrap();
    fs::create_dir_all(fixture.0.join(".git")).unwrap();
    fs::write(fixture.0.join("source.txt"), b"replacement clone").unwrap();

    let result = super::super::sync::apply_entries(
        workspace.root(),
        workspace.root_identity(),
        workspace.git_identity(),
        &snapshot.manifest,
        entries,
        1024 * 1024,
        None,
    );

    assert!(result.is_err());
    assert_eq!(
        fs::read(fixture.0.join("source.txt")).unwrap(),
        b"replacement clone"
    );
    assert_eq!(fs::read(moved.join("source.txt")).unwrap(), b"original");
    fs::remove_dir_all(moved).unwrap();
}

#[cfg(unix)]
#[test]
fn stage_out_rejects_replaced_git_metadata_identity() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned(&[("source.txt", b"container edit")]);
    let entries = super::super::archive::parse_returned(&archive, 1024 * 1024, 10).unwrap();
    let moved_git = fixture.0.join(".git-moved");
    fs::rename(fixture.0.join(".git"), &moved_git).unwrap();
    fs::create_dir(fixture.0.join(".git")).unwrap();

    let result = super::super::sync::apply_entries(
        workspace.root(),
        workspace.root_identity(),
        workspace.git_identity(),
        &snapshot.manifest,
        entries,
        1024 * 1024,
        None,
    );

    assert!(result.is_err());
    assert_eq!(fs::read(fixture.0.join("source.txt")).unwrap(), b"original");
    fs::remove_dir_all(moved_git).unwrap();
}

#[cfg(unix)]
#[test]
fn synchronization_is_idempotent_when_cancellation_and_exec_complete_together() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned(&[("source.txt", b"container edit")]);

    synchronize(&workspace, &snapshot, &archive, 1024 * 1024).unwrap();
    synchronize(&workspace, &snapshot, &archive, 1024 * 1024).unwrap();

    assert_eq!(
        fs::read(fixture.0.join("source.txt")).unwrap(),
        b"container edit"
    );
}

#[cfg(unix)]
#[test]
fn synchronization_keeps_container_outputs_readable_to_the_host_owner() {
    use std::os::unix::fs::PermissionsExt;

    // Production boundary: returned Docker archive modes become host clone
    // modes here; owner readability is required for the next scoped capture.
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned_with_mode("source.txt", b"container output", 0o004);

    synchronize(&workspace, &snapshot, &archive, 1024 * 1024).unwrap();

    let output = fixture.0.join("source.txt");
    assert_ne!(
        fs::metadata(&output).unwrap().permissions().mode() & 0o400,
        0
    );
    assert_eq!(fs::read(output).unwrap(), b"container output");
    capture_scoped(&workspace, ".", 1024 * 1024)
        .expect("the next scoped capture can read the synchronized output");
}

#[cfg(unix)]
#[test]
fn synchronization_preserves_a_host_file_created_during_execution() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    fs::write(fixture.0.join("created.txt"), b"host version").unwrap();
    let archive = returned(&[
        ("source.txt", b"original"),
        ("created.txt", b"container version"),
    ]);
    assert!(synchronize(&workspace, &snapshot, &archive, 1024 * 1024).is_err());
    assert_eq!(
        fs::read(fixture.0.join("created.txt")).unwrap(),
        b"host version"
    );
}

#[cfg(unix)]
#[test]
fn synchronization_applies_deletions_when_the_host_entry_is_unchanged() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let archive = returned(&[]);
    synchronize(&workspace, &snapshot, &archive, 1024 * 1024).unwrap();
    assert!(!fixture.0.join("source.txt").exists());
}

#[cfg(unix)]
#[test]
fn synchronization_rejects_archive_traversal_without_writing_outside_workspace() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let escape_path = fixture.0.with_extension("escape.txt");
    assert!(!escape_path.exists(), "fixture sibling path must be unique");
    let archive = traversal_returned("../escape.txt", b"bad");
    assert!(synchronize(&workspace, &snapshot, &archive, 1024 * 1024).is_err());
    assert!(!escape_path.exists());
}

#[cfg(unix)]
#[test]
fn synchronization_does_not_follow_a_replaced_parent_symlink() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("nested")).unwrap();
    fs::write(fixture.0.join("nested/file.txt"), b"original").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    let outside = fixture.0.with_extension("outside-dir");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("file.txt"), b"outside").unwrap();
    fs::remove_dir_all(fixture.0.join("nested")).unwrap();
    symlink(&outside, fixture.0.join("nested")).unwrap();
    let archive = returned(&[("nested/file.txt", b"container edit")]);
    assert!(synchronize(&workspace, &snapshot, &archive, 1024 * 1024).is_err());
    assert_eq!(fs::read(outside.join("file.txt")).unwrap(), b"outside");
    let _ = fs::remove_dir_all(outside);
}
