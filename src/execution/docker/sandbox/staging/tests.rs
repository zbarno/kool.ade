use std::{fs, io::Cursor, path::Path};

use tar::{Builder, Header};

use crate::execution::docker::sandbox::ValidatedWorkspace;

use super::{capture, capture_reviewed, capture_scoped, synchronize};

#[path = "tests/artifacts.rs"]
mod artifacts;
#[path = "tests/scoped_failure.rs"]
mod scoped_failure;

struct Fixture(std::path::PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("koolade-stage-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(path.join(".git")).unwrap();
        fs::write(path.join("source.txt"), b"original").unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn returned(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    for (path, bytes) in entries {
        let mut header = Header::new_gnu();
        header.set_entry_type(tar::EntryType::Regular);
        header.set_path(path).unwrap();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, Cursor::new(bytes)).unwrap();
    }
    builder.into_inner().unwrap()
}

fn returned_with_hard_link(path: &str, source: &str, target: &str) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    let mut file = Header::new_gnu();
    file.set_entry_type(tar::EntryType::Regular);
    file.set_path(source).unwrap();
    file.set_size(7);
    file.set_mode(0o644);
    file.set_cksum();
    builder.append(&file, Cursor::new(b"payload")).unwrap();
    let mut link = Header::new_gnu();
    link.set_entry_type(tar::EntryType::Link);
    link.set_path(path).unwrap();
    link.set_link_name(target).unwrap();
    link.set_size(0);
    link.set_mode(0o644);
    link.set_cksum();
    builder.append(&link, Cursor::new([])).unwrap();
    builder.into_inner().unwrap()
}

fn traversal_returned(path: &str, bytes: &[u8]) -> Vec<u8> {
    let mut builder = Builder::new(Vec::new());
    let mut header = Header::new_gnu();
    let name = path.as_bytes();
    assert!(name.len() < 100);
    header.as_mut_bytes()[..name.len()].copy_from_slice(name);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, Cursor::new(bytes)).unwrap();
    builder.into_inner().unwrap()
}

#[test]
fn reviewed_capture_binds_staged_bytes_to_the_pinned_git_tree() {
    let fixture = Fixture::new();
    git(&fixture.0, &["init", "--quiet"]);
    git(&fixture.0, &["config", "user.name", "Fixture"]);
    git(
        &fixture.0,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(&fixture.0, &["add", "source.txt"]);
    git(&fixture.0, &["commit", "--quiet", "-m", "reviewed source"]);
    let source_sha = git(&fixture.0, &["rev-parse", "HEAD"]);
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture_reviewed(&workspace, &source_sha, &[], None, 1024 * 1024, true)
        .expect("clean reviewed source should stage");

    let mut changed_content = snapshot.clone();
    changed_content.manifest.insert(
        "source.txt".into(),
        super::archive::Fingerprint::File {
            sha256: super::archive::hash(b"transient edit"),
            size: b"transient edit".len() as u64,
            mode: 0o644,
        },
    );
    assert!(
        super::verify_snapshot_matches_git_tree(&changed_content, &fixture.0, &source_sha, &[],)
            .is_err()
    );

    let mut missing_file = snapshot.clone();
    missing_file.manifest.remove(Path::new("source.txt"));
    assert!(
        super::verify_snapshot_matches_git_tree(&missing_file, &fixture.0, &source_sha, &[],)
            .is_err()
    );

    let mut changed_mode = snapshot;
    if let Some(super::archive::Fingerprint::File { mode, .. }) =
        changed_mode.manifest.get_mut(Path::new("source.txt"))
    {
        *mode |= 0o111;
    }
    assert!(
        super::verify_snapshot_matches_git_tree(&changed_mode, &fixture.0, &source_sha, &[],)
            .is_err()
    );
}

// Production boundary: reviewed task-clone staging must accept a generated
// file authorized only by the recipe's expected-artifact list. A path matcher
// unit test cannot prove checkout verification and snapshot comparison agree.
// This uses the local Git executable to initialize and commit one temporary
// repository; setup cost is low and Docker or network access is not required.
#[test]
fn reviewed_capture_accepts_expected_artifact_only_outputs() {
    let fixture = Fixture::new();
    git(&fixture.0, &["init", "--quiet"]);
    git(&fixture.0, &["config", "user.name", "Fixture"]);
    git(
        &fixture.0,
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(&fixture.0, &["add", "source.txt"]);
    git(&fixture.0, &["commit", "--quiet", "-m", "reviewed source"]);
    let source_sha = git(&fixture.0, &["rev-parse", "HEAD"]);
    fs::create_dir_all(fixture.0.join("dist")).unwrap();
    fs::write(fixture.0.join("dist/index.html"), b"built output").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();

    let snapshot = capture_reviewed(
        &workspace,
        &source_sha,
        &["dist/index.html".into()],
        None,
        1024 * 1024,
        true,
    )
    .expect("the expected artifact is an authorized source output");

    assert!(snapshot.manifest.contains_key(Path::new("dist/index.html")));
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

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
fn returned_archive_excludes_nested_git_metadata() {
    let archive = returned(&[("nested/.git/config", b"untrusted metadata")]);
    // Tar archives are padded to 10 KiB blocks, even when their payload is
    // tiny. Keep this above the archive size so the test reaches path filtering.
    let entries = super::archive::parse_returned(&archive, 16 * 1024, 10).unwrap();
    assert!(entries.is_empty());
}

#[cfg(unix)]
#[test]
fn returned_archive_accepts_tar_dot_slash_prefixes() {
    let archive = returned(&[("./output.txt", b"container output")]);
    let entries = super::archive::parse_returned(&archive, 16 * 1024, 10).unwrap();
    assert_eq!(
        entries.get(Path::new("output.txt")).unwrap().bytes,
        b"container output"
    );
}

#[cfg(unix)]
#[test]
fn returned_archive_materializes_bounded_hard_links_from_prior_files() {
    let archive = returned_with_hard_link(
        "./target/debug/deps/fixture",
        "./target/debug/fixture",
        "./target/debug/fixture",
    );
    let entries = super::archive::parse_returned(&archive, 16 * 1024, 10).unwrap();
    assert_eq!(
        entries
            .get(Path::new("target/debug/deps/fixture"))
            .unwrap()
            .bytes,
        b"payload"
    );
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture(&workspace, 1024 * 1024, false).unwrap();
    synchronize(&workspace, &snapshot, &archive, 1024 * 1024).unwrap();
    assert_eq!(
        fs::read(fixture.0.join("target/debug/deps/fixture")).unwrap(),
        b"payload"
    );
}

#[cfg(unix)]
#[test]
fn returned_archive_rejects_hard_links_that_escape_the_workspace() {
    let archive = returned_with_hard_link("inside", "source", "../outside");
    assert!(super::archive::parse_returned(&archive, 16 * 1024, 10).is_err());
}

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

#[cfg(unix)]
#[test]
fn scoped_stage_and_sync_cannot_modify_sibling_projects() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("apps/web")).unwrap();
    fs::create_dir_all(fixture.0.join("apps/api")).unwrap();
    fs::write(fixture.0.join("apps/web/index.html"), b"web-original").unwrap();
    fs::write(fixture.0.join("apps/api/secret.txt"), b"api-original").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture_scoped(&workspace, "apps/web", 1024 * 1024).unwrap();
    let entries = tar::Archive::new(Cursor::new(&snapshot.archive))
        .entries()
        .unwrap()
        .map(|entry| entry.unwrap().path().unwrap().into_owned())
        .collect::<Vec<_>>();
    assert!(entries.iter().any(|path| path == Path::new("index.html")));
    assert!(
        !entries
            .iter()
            .any(|path| path.starts_with("../") || path.starts_with("apps/api"))
    );

    let valid = returned(&[("index.html", b"web-updated")]);
    synchronize(&workspace, &snapshot, &valid, 1024 * 1024).unwrap();
    assert_eq!(
        fs::read(fixture.0.join("apps/web/index.html")).unwrap(),
        b"web-updated"
    );
    assert_eq!(
        fs::read(fixture.0.join("apps/api/secret.txt")).unwrap(),
        b"api-original"
    );

    let escape = traversal_returned("../api/secret.txt", b"stolen");
    assert!(synchronize(&workspace, &snapshot, &escape, 1024 * 1024).is_err());
    assert_eq!(
        fs::read(fixture.0.join("apps/api/secret.txt")).unwrap(),
        b"api-original"
    );
}

#[cfg(unix)]
#[test]
fn scoped_stage_and_sync_supports_an_explicit_project_root_target() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.0).unwrap();
    fs::write(fixture.0.join("app.txt"), b"original").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture_scoped(&workspace, ".", 1024 * 1024).unwrap();
    assert_eq!(snapshot.scope_root.as_deref(), Some(Path::new(".")));
    assert!(snapshot.manifest.contains_key(Path::new("app.txt")));
    let recovered = super::WorkspaceSnapshot::from_record_entries(
        &snapshot.record_entries(),
        snapshot.scope_root.clone(),
    )
    .unwrap();

    synchronize(
        &workspace,
        &recovered,
        &returned(&[("app.txt", b"updated")]),
        1024 * 1024,
    )
    .unwrap();
    assert_eq!(fs::read(fixture.0.join("app.txt")).unwrap(), b"updated");
    assert!(
        synchronize(
            &workspace,
            &recovered,
            &traversal_returned("../outside.txt", b"escape"),
            1024 * 1024,
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn scoped_staging_rejects_symlinked_target_roots() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let outside = fixture.0.with_extension("outside");
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, fixture.0.join("linked-app")).unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    assert!(capture_scoped(&workspace, "linked-app", 1024 * 1024).is_err());
    let _ = fs::remove_dir_all(outside);
}

#[cfg(unix)]
#[path = "tests/scoped_recovery.rs"]
mod scoped_recovery;
