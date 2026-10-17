use super::super::{WorkspaceSnapshot, archive};
use super::*;

fn record_with_manifest(
    workspace_manifest: Vec<crate::persistence::execution_records::ExecutionWorkspaceEntryV1>,
) -> crate::persistence::execution_records::ExecutionRecordV1 {
    use crate::persistence::execution_records::{
        ExecutionCleanupStateV1, ExecutionRecordStateV1, ExecutionRecordV1,
    };
    ExecutionRecordV1 {
        schema_version: 1,
        execution_id: "00000000-0000-4000-8000-000000000001".into(),
        request: None,
        project_id: "00000000-0000-4000-8000-000000000002".into(),
        task_id: "00000000-0000-4000-8000-000000000003".into(),
        target_id: "local".into(),
        controller_target_id: None,
        owner_instance_id: "00000000-0000-4000-8000-000000000004".into(),
        container_name: Some("koolade-exec-test".into()),
        workspace_lease_id: None,
        workspace_source_sha: "a".repeat(40),
        destination_sha: None,
        workspace_manifest: Some(workspace_manifest),
        container_id: None,
        command_id: None,
        command_started: false,
        result: None,
        image_digest: format!("sha256:{}", "b".repeat(64)),
        policy_version: "docker-policy-v1".into(),
        lease_generation: 1,
        lease_expires_at_unix_ms: Some(2_000),
        attempt: 1,
        state: ExecutionRecordStateV1::Preparing,
        event_cursor: 0,
        output_bytes: 0,
        started_at_unix_ms: 10,
        updated_at_unix_ms: 10,
        ended_at_unix_ms: None,
        exit_code: None,
        artifacts: Vec::new(),
        cleanup: ExecutionCleanupStateV1::Pending,
    }
}

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
    assert!(snapshot.manifest.contains_key(std::path::Path::new("apps")));
    assert!(
        snapshot
            .manifest
            .contains_key(std::path::Path::new("apps/web"))
    );
    assert!(record_with_manifest(entries.clone()).validate().is_ok());
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

    let mut forged = entries.clone();
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

    let mut forged_ancestor = entries;
    let parent = forged_ancestor
        .iter_mut()
        .find(|entry| entry.relative_path == "apps")
        .unwrap();
    parent.kind = crate::persistence::execution_records::ExecutionWorkspaceEntryKindV1::File;
    parent.sha256 = Some("0".repeat(64));
    parent.size_bytes = 1;
    assert!(
        WorkspaceSnapshot::from_record_entries(&forged_ancestor, snapshot.scope_root.clone())
            .is_err()
    );
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
