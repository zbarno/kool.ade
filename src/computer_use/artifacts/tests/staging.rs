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

#[cfg(unix)]
#[test]
fn task_build_staging_accepts_opaque_extensionless_outputs_only_for_guest_transfer() {
    let (_temp, bridge) = bridge();
    let identity = identity();
    let task = tempfile::tempdir().unwrap();
    fs::create_dir_all(task.path().join("build")).unwrap();
    fs::write(task.path().join("build/editor"), b"opaque build output").unwrap();
    let grant = bridge
        .issue_grant(grant_request(
            &identity,
            ComputerArtifactFlowV1::TaskArtifactToGuest,
            "build/editor",
            false,
        ))
        .unwrap();
    let receipt = bridge
        .stage_task_file(&grant.grant_id, task.path(), REV, &identity, NOW + 1)
        .unwrap();
    assert_eq!(receipt.media_type, "application/octet-stream");
    let (manifest, loaded) = bridge
        .load_for_guest(&receipt.artifact_id, &identity, REV, 3, 2, NOW + 2)
        .unwrap();
    assert_eq!(manifest.relative_path, "build/editor");
    assert_eq!(loaded, b"opaque build output");
}

#[test]
fn approved_task_build_bytes_are_digest_and_generation_bound_and_one_use() {
    let (_temp, bridge) = bridge();
    let identity = identity();
    let bytes = b"approved opaque executable";
    let digest = super::super::validation::sha256(bytes);
    let approval_sha256 = "c".repeat(64);
    let mut request = grant_request(
        &identity,
        ComputerArtifactFlowV1::TaskArtifactToGuest,
        "build/editor",
        false,
    );
    request.max_bytes = bytes.len() as u64;
    let grant = bridge
        .issue_approved_task_app_grant(request, approval_sha256.clone())
        .unwrap();
    assert!(
        bridge
            .stage_approved_task_build_bytes(
                &grant.grant_id,
                &identity,
                REV,
                "build/editor",
                &"d".repeat(64),
                3,
                2,
                &approval_sha256,
                bytes,
                NOW + 1,
            )
            .is_err()
    );
    let receipt = bridge
        .stage_approved_task_build_bytes(
            &grant.grant_id,
            &identity,
            REV,
            "build/editor",
            &digest,
            3,
            2,
            &approval_sha256,
            bytes,
            NOW + 1,
        )
        .unwrap();
    assert_eq!(receipt.size_bytes, bytes.len() as u64);
    assert!(
        bridge
            .stage_approved_task_build_bytes(
                &grant.grant_id,
                &identity,
                REV,
                "build/editor",
                &digest,
                3,
                2,
                &approval_sha256,
                bytes,
                NOW + 2,
            )
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn staged_task_application_loads_only_its_exact_sorted_p15_receipts() {
    use crate::computer_use::apps::{
        GuestAppArchitectureV1, StagedTaskApplicationV1, StagedTaskArtifactV1,
    };
    use sha2::{Digest, Sha256};

    let (_temp, bridge) = bridge();
    let identity = identity();
    let task = tempfile::tempdir().unwrap();
    fs::create_dir_all(task.path().join("build")).unwrap();
    fs::write(task.path().join("build/app"), b"ELF bytes").unwrap();
    fs::write(task.path().join("build/config.json"), b"{}").unwrap();
    let mut artifacts = Vec::new();
    for path in ["build/app", "build/config.json"] {
        let grant = bridge
            .issue_approved_task_app_grant(
                grant_request(
                    &identity,
                    ComputerArtifactFlowV1::TaskArtifactToGuest,
                    path,
                    false,
                ),
                "c".repeat(64),
            )
            .unwrap();
        let receipt = bridge
            .stage_task_file(&grant.grant_id, task.path(), REV, &identity, NOW + 1)
            .unwrap();
        artifacts.push(StagedTaskArtifactV1 {
            relative_path: path.into(),
            transfer_grant_id: grant.grant_id,
            receipt,
        });
    }
    let manifest = artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.relative_path.as_str(),
                artifact.receipt.sha256.strip_prefix("sha256:").unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let build_receipt_sha256 = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&manifest).unwrap())
    );
    let application = StagedTaskApplicationV1 {
        schema_version: 1,
        app_id: crate::computer_use::mcp::COMPUTER_USE_RUNTIME_QA_APP_ID_V1.into(),
        executable_id: "app".into(),
        executable_relative_path: "build/app".into(),
        task_revision: REV.into(),
        app_target_id: "target".into(),
        execution_recipe_id: "recipe".into(),
        build_receipt_sha256,
        build_approval_sha256: "c".repeat(64),
        guest_image_digest: format!("sha256:{}", "a".repeat(64)),
        guest_architecture: GuestAppArchitectureV1::X86_64,
        artifacts,
    };
    let contents = bridge
        .load_staged_task_application_for_guest(&application, &identity, REV, 3, 2, NOW + 2)
        .unwrap();
    assert_eq!(contents.len(), 2);
    assert_eq!(contents[0].bytes, b"ELF bytes");
    assert_eq!(contents[1].bytes, b"{}");

    let mut wrong_grant = application.clone();
    wrong_grant.artifacts[0].transfer_grant_id = uuid::Uuid::new_v4().to_string();
    assert!(
        bridge
            .load_staged_task_application_for_guest(&wrong_grant, &identity, REV, 3, 2, NOW + 2)
            .is_err()
    );

    let mut wrong_approval = application.clone();
    wrong_approval.build_approval_sha256 = "d".repeat(64);
    assert!(
        bridge
            .load_staged_task_application_for_guest(&wrong_approval, &identity, REV, 3, 2, NOW + 2,)
            .is_err()
    );
}
