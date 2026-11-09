use super::*;

#[test]
fn artifact_digest_path_and_mime_are_checked_and_revoke_removes_bytes() {
    let (_temp, bridge) = bridge();
    let identity = identity();
    let grant = bridge
        .issue_grant(grant_request(
            &identity,
            ComputerArtifactFlowV1::GuestArtifactToTask,
            "out/result.txt",
            false,
        ))
        .unwrap();
    let bytes = b"content";
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
                "sha256:bad",
                NOW + 1
            )
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
                "result.bin",
                bytes,
                &super::super::validation::sha256(bytes),
                NOW + 1
            )
            .is_err()
    );

    let grant = bridge
        .issue_grant(grant_request(
            &identity,
            ComputerArtifactFlowV1::GuestArtifactToTask,
            "out/valid.txt",
            false,
        ))
        .unwrap();
    let receipt = bridge
        .receive_guest_file(
            &grant.grant_id,
            &identity,
            REV,
            3,
            2,
            "valid.txt",
            bytes,
            &super::super::validation::sha256(bytes),
            NOW + 1,
        )
        .unwrap();
    bridge.revoke(&grant.grant_id, &identity).unwrap();
    assert!(
        !bridge
            .store
            .read(format!(
                "computer-use/transfers/v1/blobs/{}.bin",
                receipt.artifact_id
            ))
            .is_ok()
    );
    assert!(
        bridge
            .load_for_task_apply(&receipt.artifact_id, &identity, REV, 3, 2, NOW + 2)
            .is_err()
    );
}

#[test]
fn unicode_case_collisions_are_rejected_and_stale_grants_expire() {
    let (_temp, bridge) = bridge();
    let identity = identity();
    let first = bridge
        .issue_grant(grant_request(
            &identity,
            ComputerArtifactFlowV1::GuestArtifactToTask,
            "out/straße.txt",
            false,
        ))
        .unwrap();
    let one = b"one";
    bridge
        .receive_guest_file(
            &first.grant_id,
            &identity,
            REV,
            3,
            2,
            "straße.txt",
            one,
            &super::super::validation::sha256(one),
            NOW + 1,
        )
        .unwrap();
    let second = bridge
        .issue_grant(grant_request(
            &identity,
            ComputerArtifactFlowV1::GuestArtifactToTask,
            "out/STRASSE.txt",
            false,
        ))
        .unwrap();
    let two = b"two";
    assert!(
        bridge
            .receive_guest_file(
                &second.grant_id,
                &identity,
                REV,
                3,
                2,
                "STRASSE.txt",
                two,
                &super::super::validation::sha256(two),
                NOW + 2
            )
            .is_err()
    );
    assert!(
        bridge
            .receive_guest_file(
                &second.grant_id,
                &identity,
                REV,
                3,
                2,
                "STRASSE.txt",
                two,
                &super::super::validation::sha256(two),
                NOW + 70_001
            )
            .is_err()
    );
}

#[test]
fn only_explicit_download_grant_can_export_without_overwrite() {
    let (temp, bridge) = bridge();
    let owner = identity();
    let grant = bridge
        .issue_grant(grant_request(
            &owner,
            ComputerArtifactFlowV1::UserDownloadFromGuest,
            "downloads/result.txt",
            true,
        ))
        .unwrap();
    let bytes = b"download";
    let receipt = bridge
        .receive_guest_file(
            &grant.grant_id,
            &owner,
            REV,
            3,
            2,
            "result.txt",
            bytes,
            &super::super::validation::sha256(bytes),
            NOW + 1,
        )
        .unwrap();
    let output = temp.path().join("exported.txt");
    bridge
        .export_to_user(&receipt.artifact_id, &owner, &output, NOW + 2)
        .unwrap();
    assert_eq!(fs::read(&output).unwrap(), bytes);
    assert!(
        bridge
            .export_to_user(&receipt.artifact_id, &owner, &output, NOW + 2)
            .is_err()
    );
    assert!(
        bridge
            .export_to_user(
                &receipt.artifact_id,
                &identity(),
                &temp.path().join("wrong.txt"),
                NOW + 2
            )
            .is_err()
    );
}

#[test]
fn personal_guest_file_transfer_fails_closed_until_personal_sessions_exist() {
    let (_temp, bridge) = bridge();
    let identity = identity();
    let request = grant_request(
        &identity,
        ComputerArtifactFlowV1::UserApprovedFileToPersonalGuest,
        "Documents/private.txt",
        true,
    );
    assert!(bridge.issue_grant(request).is_err());
}

#[test]
fn artifact_listing_and_application_are_isolated_to_the_issuing_task_identity() {
    let (_temp, bridge) = bridge();
    let owner = identity();
    let grant = bridge
        .issue_grant(grant_request(
            &owner,
            ComputerArtifactFlowV1::GuestArtifactToTask,
            "out/result.txt",
            false,
        ))
        .unwrap();
    let bytes = b"result";
    let receipt = bridge
        .receive_guest_file(
            &grant.grant_id,
            &owner,
            REV,
            3,
            2,
            "result.txt",
            bytes,
            &super::super::validation::sha256(bytes),
            NOW + 1,
        )
        .unwrap();

    let mut other_task = owner.clone();
    other_task.task_id = "00000000-0000-4000-8000-000000000099".into();
    let mut other_user = owner.clone();
    other_user.user_id = "00000000-0000-4000-8000-000000000098".into();
    assert!(
        bridge
            .list_for_identity(&other_task, NOW + 2)
            .unwrap()
            .is_empty()
    );
    assert!(
        bridge
            .list_for_identity(&other_user, NOW + 2)
            .unwrap()
            .is_empty()
    );
    assert!(
        bridge
            .load_for_task_apply(&receipt.artifact_id, &other_task, REV, 3, 2, NOW + 2)
            .is_err()
    );
    assert!(
        bridge
            .load_for_task_apply(&receipt.artifact_id, &other_user, REV, 3, 2, NOW + 2)
            .is_err()
    );
}
