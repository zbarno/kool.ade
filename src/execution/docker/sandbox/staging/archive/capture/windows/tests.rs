use std::{
    io::Read,
    path::{Path, PathBuf},
};

use super::{WorkspaceObjectId, capture, capture_excluding, native, workspace_identity};

fn init_git(root: &Path) {
    git(root, &["init", "--quiet"]);
    git(root, &["config", "--local", "core.autocrlf", "false"]);
}

fn git(root: &Path, args: &[&str]) -> std::process::Output {
    let output = crate::core::planning_sync::validated_git_command(root)
        .unwrap()
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

// These tests exercise the production capture boundary against native Windows
// temporary directories. Parser-only tests cannot prove handle-relative opens,
// share-mode locking, reparse rejection, or Windows hard-link behavior. They
// need no Docker or network service and have low setup cost.

fn identity(path: &Path) -> WorkspaceObjectId {
    let metadata = std::fs::symlink_metadata(path).unwrap();
    workspace_identity::from_metadata(&metadata, path).unwrap()
}

#[test]
fn capture_preserves_raw_bytes_and_uses_non_executable_windows_mode() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    let bytes = b"first\r\nsecond\r\n\0raw";
    std::fs::write(root.path().join("raw.txt"), bytes).unwrap();
    std::fs::write(root.path().join(".git/private"), b"private").unwrap();

    let snapshot = capture(
        root.path(),
        &identity(root.path()),
        None,
        1024 * 1024,
        false,
        32,
    )
    .unwrap();
    assert!(
        !snapshot
            .manifest
            .contains_key(&PathBuf::from(".git/private"))
    );
    let mut archive = tar::Archive::new(snapshot.archive.as_slice());
    let mut found = false;
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.path().unwrap().as_ref() == Path::new("raw.txt") {
            assert_eq!(entry.header().mode().unwrap(), 0o644);
            let mut captured = Vec::new();
            entry.read_to_end(&mut captured).unwrap();
            assert_eq!(captured, bytes);
            found = true;
        }
    }
    assert!(found);
}

#[test]
fn scoped_capture_stays_under_the_root_handle_scope() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    std::fs::create_dir(root.path().join("selected")).unwrap();
    std::fs::create_dir(root.path().join("outside")).unwrap();
    std::fs::write(root.path().join("selected/inside.txt"), b"inside").unwrap();
    std::fs::write(root.path().join("outside/secret.txt"), b"outside").unwrap();

    let scope = Path::new("selected");
    let snapshot = capture(
        root.path(),
        &identity(root.path()),
        Some(scope),
        1024 * 1024,
        false,
        32,
    )
    .unwrap();
    assert_eq!(snapshot.manifest.len(), 1);
    assert!(snapshot.manifest.contains_key(Path::new("inside.txt")));
    assert!(
        !snapshot
            .manifest
            .contains_key(Path::new("../outside/secret.txt"))
    );
}

#[test]
fn capture_rejects_a_replaced_workspace_identity() {
    let first = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(first.path().join("data.txt"), b"first").unwrap();
    assert!(
        capture_excluding(
            first.path(),
            &identity(other.path()),
            None,
            1024 * 1024,
            false,
            32,
            &[],
        )
        .is_err()
    );
}

#[test]
fn capture_rejects_hard_linked_files() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    let original = root.path().join("original.txt");
    std::fs::write(&original, b"linked").unwrap();
    std::fs::hard_link(&original, root.path().join("alias.txt")).unwrap();
    assert!(
        capture(
            root.path(),
            &identity(root.path()),
            None,
            1024 * 1024,
            false,
            32
        )
        .is_err()
    );
}

#[test]
fn held_root_and_ancestor_handles_block_workspace_rename() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("workspace");
    let moved = parent.path().join("moved");
    std::fs::create_dir(&root).unwrap();
    let _handles = native::open_root(&root).unwrap();
    assert!(std::fs::rename(&root, &moved).is_err());
}

#[test]
fn capture_rejects_reparse_entries_without_following_them() {
    use std::os::windows::fs::symlink_file;

    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), b"outside").unwrap();
    let link = root.path().join("linked.txt");
    if let Err(error) = symlink_file(outside.path().join("secret.txt"), &link) {
        if error.kind() == std::io::ErrorKind::PermissionDenied
            || error.raw_os_error() == Some(1314)
        {
            return;
        }
        panic!("could not create reparse-point fixture: {error}");
    }
    assert!(
        capture(
            root.path(),
            &identity(root.path()),
            None,
            1024 * 1024,
            false,
            32
        )
        .is_err()
    );
}

#[test]
fn tracked_git_modes_are_preserved_in_archive_and_manifest() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    std::fs::write(root.path().join("run.sh"), b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::write(root.path().join("plain.txt"), b"plain\n").unwrap();
    git(root.path(), &["add", "--", "run.sh", "plain.txt"]);
    git(root.path(), &["update-index", "--chmod=+x", "--", "run.sh"]);

    let snapshot = capture(
        root.path(),
        &identity(root.path()),
        None,
        1024 * 1024,
        false,
        32,
    )
    .unwrap();
    assert!(matches!(
        snapshot.manifest.get(Path::new("run.sh")),
        Some(super::super::super::Fingerprint::File { mode: 0o755, .. })
    ));
    assert!(matches!(
        snapshot.manifest.get(Path::new("plain.txt")),
        Some(super::super::super::Fingerprint::File { mode: 0o644, .. })
    ));

    let mut archive = tar::Archive::new(snapshot.archive.as_slice());
    let modes = archive
        .entries()
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.path().unwrap().to_string_lossy().replace('\\', "/"),
                entry.header().mode().unwrap(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(modes.get("run.sh"), Some(&0o755));
    assert_eq!(modes.get("plain.txt"), Some(&0o644));
}

#[test]
fn scoped_capture_uses_root_relative_git_mode() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    std::fs::create_dir(root.path().join("selected")).unwrap();
    std::fs::write(root.path().join("selected/run.sh"), b"#!/bin/sh\n").unwrap();
    std::fs::write(root.path().join("outside.txt"), b"outside").unwrap();
    git(
        root.path(),
        &["add", "--", "selected/run.sh", "outside.txt"],
    );
    git(
        root.path(),
        &["update-index", "--chmod=+x", "--", "selected/run.sh"],
    );

    let snapshot = capture(
        root.path(),
        &identity(root.path()),
        Some(Path::new("selected")),
        1024 * 1024,
        false,
        32,
    )
    .unwrap();
    assert!(matches!(
        snapshot.manifest.get(Path::new("run.sh")),
        Some(super::super::super::Fingerprint::File { mode: 0o755, .. })
    ));
}

#[test]
fn capture_rejects_case_only_index_name_changes() {
    let root = tempfile::tempdir().unwrap();
    init_git(root.path());
    std::fs::write(root.path().join("Tool.sh"), b"#!/bin/sh\n").unwrap();
    git(root.path(), &["add", "--", "Tool.sh"]);
    std::fs::rename(root.path().join("Tool.sh"), root.path().join("middle.tmp")).unwrap();
    std::fs::rename(root.path().join("middle.tmp"), root.path().join("tool.sh")).unwrap();

    assert!(
        capture(
            root.path(),
            &identity(root.path()),
            None,
            1024 * 1024,
            false,
            32,
        )
        .is_err()
    );
}
