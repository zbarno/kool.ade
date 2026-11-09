use super::*;

#[test]
fn guest_artifact_is_private_revision_bound_and_read_only_after_fencing() {
    let (_temp, bridge) = bridge();
    let identity = identity();
    let grant = bridge
        .issue_grant(grant_request(
            &identity,
            ComputerArtifactFlowV1::GuestArtifactToTask,
            "outputs/result.txt",
            false,
        ))
        .unwrap();
    let bytes = b"safe result";
    let receipt = bridge
        .receive_guest_file(
            &grant.grant_id,
            &identity,
            REV,
            3,
            2,
            "result.txt",
            bytes,
            &super::super::validation::sha256(bytes),
            NOW + 1,
        )
        .unwrap();
    let (manifest, loaded) = bridge
        .load_for_task_apply(&receipt.artifact_id, &identity, REV, 3, 2, NOW + 2)
        .unwrap();
    assert_eq!(loaded, bytes);
    assert_eq!(manifest.relative_path, "outputs/result.txt");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let blob = bridge.store.local_root().join(format!(
            "computer-use/transfers/v1/blobs/{}.bin",
            receipt.artifact_id
        ));
        assert_eq!(
            fs::metadata(&blob).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let directory = bridge
            .store
            .local_root()
            .join("computer-use/transfers/v1/blobs");
        assert_eq!(
            fs::metadata(directory).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
    assert!(
        bridge
            .load_for_task_apply(
                &receipt.artifact_id,
                &identity,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                3,
                2,
                NOW + 2
            )
            .is_err()
    );
    assert!(
        bridge
            .load_for_task_apply(&receipt.artifact_id, &identity, REV, 4, 2, NOW + 2)
            .is_err()
    );
    assert!(
        bridge
            .receive_guest_file(
                &grant.grant_id,
                &identity,
                REV,
                3,
                2,
                "result.txt",
                bytes,
                &receipt.sha256,
                NOW + 2
            )
            .is_err()
    );
}

/// Exercises explicit file staging against real filesystem metadata. A byte
/// validator cannot detect source symlinks, hard links, private path names, or
/// ancestor substitution. Uses a temp task checkout only; no external services.
#[cfg(unix)]
#[test]
fn task_file_staging_rejects_private_paths_symlinks_and_hardlinks() {
    use std::os::unix::fs::symlink;
    let (_temp, bridge) = bridge();
    let identity = identity();
    let task = tempfile::tempdir().unwrap();
    fs::create_dir_all(task.path().join("src")).unwrap();
    fs::write(task.path().join("src/readme.txt"), b"hello").unwrap();
    let grant = bridge
        .issue_grant(grant_request(
            &identity,
            ComputerArtifactFlowV1::TaskArtifactToGuest,
            "src/readme.txt",
            false,
        ))
        .unwrap();
    let staged = bridge
        .stage_task_file(&grant.grant_id, task.path(), REV, &identity, NOW + 1)
        .unwrap();
    let (_, guest_bytes) = bridge
        .load_for_guest(&staged.artifact_id, &identity, REV, 3, 2, NOW + 2)
        .unwrap();
    assert_eq!(guest_bytes, b"hello");
    assert!(
        bridge
            .load_for_guest(
                &staged.artifact_id,
                &identity,
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                3,
                2,
                NOW + 2
            )
            .is_err()
    );

    fs::write(task.path().join(".env"), b"secret").unwrap();
    let private = bridge
        .issue_grant(grant_request(
            &identity,
            ComputerArtifactFlowV1::TaskArtifactToGuest,
            ".env",
            false,
        ))
        .unwrap();
    assert!(
        bridge
            .stage_task_file(&private.grant_id, task.path(), REV, &identity, NOW + 1)
            .is_err()
    );

    symlink(
        task.path().join("src/readme.txt"),
        task.path().join("src/alias.txt"),
    )
    .unwrap();
    let linked = bridge
        .issue_grant(grant_request(
            &identity,
            ComputerArtifactFlowV1::TaskArtifactToGuest,
            "src/alias.txt",
            false,
        ))
        .unwrap();
    assert!(
        bridge
            .stage_task_file(&linked.grant_id, task.path(), REV, &identity, NOW + 1)
            .is_err()
    );

    fs::hard_link(
        task.path().join("src/readme.txt"),
        task.path().join("src/hardlink.txt"),
    )
    .unwrap();
    let hardlink = bridge
        .issue_grant(grant_request(
            &identity,
            ComputerArtifactFlowV1::TaskArtifactToGuest,
            "src/hardlink.txt",
            false,
        ))
        .unwrap();
    assert!(
        bridge
            .stage_task_file(&hardlink.grant_id, task.path(), REV, &identity, NOW + 1)
            .is_err()
    );
}
