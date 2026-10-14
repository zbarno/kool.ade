use super::super::{WorkspaceSnapshot, archive};
use super::*;

#[test]
fn scoped_snapshot_recovery_keeps_the_persisted_write_boundary() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("apps/web")).unwrap();
    fs::create_dir_all(fixture.0.join("apps/api")).unwrap();
    fs::write(fixture.0.join("apps/web/index.html"), b"web").unwrap();
    fs::write(fixture.0.join("apps/api/secret.txt"), b"api").unwrap();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let snapshot = capture_scoped(&workspace, "apps/web", 1024 * 1024).unwrap();
    let entries = snapshot.record_entries();
    let recovered =
        WorkspaceSnapshot::from_record_entries(&entries, snapshot.scope_root.clone()).unwrap();
    synchronize(
        &workspace,
        &recovered,
        &returned(&[("index.html", b"updated")]),
        1024 * 1024,
    )
    .unwrap();
    assert_eq!(
        fs::read(fixture.0.join("apps/api/secret.txt")).unwrap(),
        b"api"
    );

    let mut forged = entries;
    forged.push(
        crate::persistence::execution_records::ExecutionWorkspaceEntryV1 {
            relative_path: "apps/api/secret.txt".into(),
            kind: crate::persistence::execution_records::ExecutionWorkspaceEntryKindV1::File,
            sha256: Some("0".repeat(64)),
            size_bytes: 3,
            mode: 0o644,
        },
    );
    assert!(WorkspaceSnapshot::from_record_entries(&forged, snapshot.scope_root.clone(),).is_err());
}

#[cfg(unix)]
#[test]
fn returned_symlink_and_hard_link_entries_are_rejected() {
    fn link_archive(kind: tar::EntryType) -> Vec<u8> {
        let mut builder = Builder::new(Vec::new());
        let mut header = Header::new_gnu();
        header.set_entry_type(kind);
        header.set_path("link").unwrap();
        header.set_link_name("target").unwrap();
        header.set_size(0);
        header.set_cksum();
        builder.append(&header, Cursor::new([])).unwrap();
        builder.into_inner().unwrap()
    }
    for kind in [tar::EntryType::Symlink, tar::EntryType::Link] {
        assert!(archive::parse_returned(&link_archive(kind), 1024, 10).is_err());
    }
}

#[cfg(unix)]
#[test]
fn returned_archive_rejects_file_parent_collisions() {
    let archive = returned(&[("node", b"file"), ("node/child", b"child")]);
    assert!(archive::parse_returned(&archive, 1024, 10).is_err());
}
