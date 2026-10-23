use super::*;

#[cfg(unix)]
#[test]
fn capture_excludes_git_and_sync_applies_changes_through_directory_handles() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("nested/.git")).unwrap();
    fs::write(fixture.0.join("nested/.git/config"), b"nested repository").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    assert!(snapshot.manifest.contains_key(Path::new("source.txt")));
    assert!(
        !snapshot
            .manifest
            .keys()
            .any(|path| path.starts_with(".git"))
    );

    let archive = returned(&[("source.txt", b"updated"), ("created.txt", b"new")]);
    synchronize(&workspace, &snapshot, &archive, 1024 * 1024).unwrap();
    assert_eq!(fs::read(fixture.0.join("source.txt")).unwrap(), b"updated");
    assert_eq!(fs::read(fixture.0.join("created.txt")).unwrap(), b"new");
}

#[cfg(unix)]
#[test]
fn read_only_snapshot_makes_nested_files_and_directories_non_writable() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("nested/deeper")).unwrap();
    fs::write(fixture.0.join("nested/deeper/source.txt"), b"read only").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, true).unwrap();
    let mut archive = tar::Archive::new(Cursor::new(snapshot.archive));
    let mut checked_nested_file = false;
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        let path = entry.path().unwrap().into_owned();
        if path.starts_with("nested") {
            let mode = entry.header().mode().unwrap();
            assert_eq!(mode & 0o222, 0, "{path:?} must not be writable");
            assert_eq!(
                entry.header().uid().unwrap(),
                0,
                "{path:?} stays root-owned"
            );
            assert_eq!(
                entry.header().gid().unwrap(),
                0,
                "{path:?} stays root-owned"
            );
            checked_nested_file |= path == Path::new("nested/deeper/source.txt");
        }
    }
    assert!(checked_nested_file);
}

#[cfg(unix)]
#[test]
fn capture_rejects_symlinked_and_hard_linked_source_files() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = fixture.0.with_extension("outside");
    fs::write(&outside, b"outside").unwrap();
    symlink(&outside, fixture.0.join("linked.txt")).unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    assert!(capture(&workspace, 1024 * 1024, false).is_err());
    fs::remove_file(fixture.0.join("linked.txt")).unwrap();
    fs::hard_link(&outside, fixture.0.join("hardlinked.txt")).unwrap();
    assert!(capture(&workspace, 1024 * 1024, false).is_err());
    let _ = fs::remove_file(outside);
}

#[cfg(unix)]
#[test]
fn scoped_capture_round_trips_cargo_target_paths_over_tar_name_limit() {
    let fixture = Fixture::new();
    let relative = Path::new(
        "apps/web/evidence/.koolade-target/debug/.fingerprint/stack_fixture-68bc9dc8f0382166/test-bin-stack_fixture.json",
    );
    let file = fixture.0.join(relative);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"synthetic cargo target metadata").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture_scoped(&workspace, "apps/web", 1024 * 1024).unwrap();
    let entries =
        super::super::archive::parse_returned(&snapshot.archive, 1024 * 1024, 32).unwrap();
    assert!(entries.contains_key(Path::new(
        "evidence/.koolade-target/debug/.fingerprint/stack_fixture-68bc9dc8f0382166/test-bin-stack_fixture.json"
    )));
}
