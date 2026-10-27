// Every apply fixture uses local Git because production stage-out reads the
// real index. Git is the only external dependency; no Docker/network is used.
use std::{collections::BTreeMap, fs};

use super::super::super::archive::Manifest;
use super::Checkpoint;

#[path = "tests/modes.rs"]
mod modes;
#[path = "tests/pinning.rs"]
mod pinning;
#[path = "tests/support.rs"]
mod support;
use support::*;

#[test]
// Production boundary: this drives stage-out through native Windows handle
// operations, including the destination name collision. A matcher unit test
// cannot prove the no-replace rename preserves the concurrent host file. It
// uses only a local temp directory; no Docker or network is needed.
fn new_file_collision_preserves_host_content() {
    let fixture = tempfile::tempdir().unwrap();
    let update = returned("new.txt", b"container");
    let result = apply_test(
        fixture.path(),
        &Manifest::new(),
        &update,
        None,
        |checkpoint| {
            if let Checkpoint::BeforeCreateInstall { target, .. } = checkpoint {
                fs::write(fixture.path().join(target), b"host-created").unwrap();
            }
            false
        },
    );

    assert!(result.is_err());
    assert_eq!(
        fs::read(fixture.path().join("new.txt")).unwrap(),
        b"host-created"
    );
    assert!(
        !fs::read_dir(fixture.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".koolade-sync-"))
    );
}

#[test]
// Production boundary: a host file created during the replacement visibility
// gap must not be overwritten, and rollback failure must retain the displaced
// baseline as a unique conflict file. Uses real local NTFS/Windows renames;
// no Docker or network is required.
fn replacement_collision_preserves_host_and_displaced_baseline() {
    let fixture = tempfile::tempdir().unwrap();
    fs::write(fixture.path().join("source.txt"), b"baseline").unwrap();
    let result = apply_test(
        fixture.path(),
        &baseline("source.txt", b"baseline"),
        &returned("source.txt", b"container"),
        None,
        |checkpoint| {
            if let Checkpoint::AfterReplaceAside { target, .. } = checkpoint {
                fs::write(fixture.path().join(target), b"host-created").unwrap();
            }
            false
        },
    );

    assert!(result.is_err());
    assert_eq!(
        fs::read(fixture.path().join("source.txt")).unwrap(),
        b"host-created"
    );
    assert!(contains_recovery_file(
        fixture.path(),
        ".koolade-conflict-",
        b"baseline"
    ));
}

#[test]
// Production boundary: if the staged replacement disappears after the old
// name is moved aside, rollback must restore the old file without replacing a
// destination. This exercises real handle-relative NT renames on Windows.
fn replacement_failure_restores_previous_file_when_destination_is_free() {
    let fixture = tempfile::tempdir().unwrap();
    fs::write(fixture.path().join("source.txt"), b"baseline").unwrap();
    let result = apply_test(
        fixture.path(),
        &baseline("source.txt", b"baseline"),
        &returned("source.txt", b"container"),
        None,
        |checkpoint| matches!(checkpoint, Checkpoint::AfterReplaceAside { .. }),
    );

    assert!(result.is_err());
    assert_eq!(
        fs::read(fixture.path().join("source.txt")).unwrap(),
        b"baseline"
    );
    assert!(!contains_recovery_file(
        fixture.path(),
        ".koolade-conflict-",
        b"baseline"
    ));
}

#[test]
// Production boundary: a successful stage-out must install the container file
// through the same handle-relative transaction and remove its old version.
// Uses only a local workspace; no Docker or network setup is needed.
fn replacement_installs_container_content_and_cleans_backup() {
    let fixture = tempfile::tempdir().unwrap();
    fs::write(fixture.path().join("source.txt"), b"baseline").unwrap();

    apply_test(
        fixture.path(),
        &baseline("source.txt", b"baseline"),
        &returned("source.txt", b"container"),
        None,
        |_| false,
    )
    .unwrap();

    assert_eq!(
        fs::read(fixture.path().join("source.txt")).unwrap(),
        b"container"
    );
    assert!(
        !fs::read_dir(fixture.path())
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name().to_string_lossy().starts_with(".koolade-"))
    );
}

#[test]
// Production boundary: a host file created while a container deletion is
// staged must survive while the unchanged baseline is removed. A pure helper
// test cannot verify Windows handle-relative deletion. Uses a local directory.
fn deletion_collision_preserves_new_host_file() {
    let fixture = tempfile::tempdir().unwrap();
    fs::write(fixture.path().join("source.txt"), b"baseline").unwrap();
    let result = apply_test(
        fixture.path(),
        &baseline("source.txt", b"baseline"),
        &BTreeMap::new(),
        None,
        |checkpoint| {
            if let Checkpoint::AfterDeleteAside { target, .. } = checkpoint {
                fs::write(fixture.path().join(target), b"host-created").unwrap();
            }
            false
        },
    );

    result.unwrap();
    assert_eq!(
        fs::read(fixture.path().join("source.txt")).unwrap(),
        b"host-created"
    );
}

#[test]
// Production boundary: the full stage-out preflight must reject an unapproved
// output before touching host files. This uses the real Windows apply path and
// local synthetic manifest data; no Docker or network setup is required.
fn unapproved_output_is_rejected_before_write() {
    let fixture = tempfile::tempdir().unwrap();
    fs::write(fixture.path().join("source.txt"), b"baseline").unwrap();
    let result = apply_test(
        fixture.path(),
        &baseline("source.txt", b"baseline"),
        &returned("source.txt", b"container"),
        Some(&["dist/result.txt".into()]),
        |_| panic!("output policy should reject before synchronization"),
    );

    assert!(result.is_err());
    assert_eq!(
        fs::read(fixture.path().join("source.txt")).unwrap(),
        b"baseline"
    );
}

#[test]
// Production boundary: the opened root handle must match the identity captured
// by `ValidatedWorkspace`, even when the caller supplies a different path.
// Uses two local temporary directories and no Docker/network services.
fn workspace_root_identity_mismatch_is_rejected_before_write() {
    let expected_root = tempfile::tempdir().unwrap();
    let replacement_root = tempfile::tempdir().unwrap();
    let expected_identity = identity(expected_root.path());
    let update = returned("new.txt", b"container");

    let result = super::apply_with_hook(
        replacement_root.path(),
        &expected_identity,
        &expected_identity,
        &Manifest::new(),
        &update,
        BYTE_LIMIT,
        None,
        |_| panic!("root identity must fail before synchronization"),
    );

    assert!(result.is_err_and(|error| error.contains("root changed")));
    assert!(!replacement_root.path().join("new.txt").exists());
}

#[test]
// Production boundary: the Git index handle must match the Git metadata
// identity captured by `ValidatedWorkspace`. A root-only test cannot prove
// stage-out refuses a replaced `.git`; requires local Git and temp directories.
fn git_metadata_identity_mismatch_is_rejected_before_write() {
    let fixture = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    initialize_git(fixture.path());
    let root_identity = identity(fixture.path());
    let wrong_git_identity = identity(other.path());
    let update = returned("new.txt", b"container");

    let result = super::apply_with_hook(
        fixture.path(),
        &root_identity,
        &wrong_git_identity,
        &Manifest::new(),
        &update,
        BYTE_LIMIT,
        None,
        |_| panic!("Git metadata identity must fail before synchronization"),
    );

    assert!(result.is_err_and(|error| error.contains("Git metadata changed")));
    assert!(!fixture.path().join("new.txt").exists());
}

#[test]
// Production boundary: traversal through a workspace junction or symlink must
// stop before stage-out reaches an outside directory. This uses a local
// symlink and synthetic archive; Windows developer mode or the symlink privilege
// is required to create the fixture, so restricted hosts skip it.
fn reparse_parent_is_rejected_without_writing_outside_the_workspace() {
    use std::os::windows::fs::symlink_dir;

    let fixture = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let link = fixture.path().join("jump");
    if symlink_dir(outside.path(), &link).is_err() {
        return;
    }
    let update = returned("jump/escape.txt", b"container");

    let result = apply_test(fixture.path(), &Manifest::new(), &update, None, |_| false);

    assert!(result.is_err());
    assert!(!outside.path().join("escape.txt").exists());
}
