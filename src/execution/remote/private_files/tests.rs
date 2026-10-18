use super::*;

fn grant() -> RemotePrivateFileGrantV1 {
    RemotePrivateFileGrantV1 {
        grant_id: "00000000-0000-4000-8000-000000000141".into(),
        controller_id: "00000000-0000-4000-8000-000000000142".into(),
        operator_id: "synthetic-operator-141".into(),
        worker_id: "00000000-0000-4000-8000-000000000143".into(),
        worker_host_key_fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
        workspace: WorkspaceRef {
            task_id: "00000000-0000-4000-8000-000000000144".into(),
            project_id: "00000000-0000-4000-8000-000000000145".into(),
            source_sha: "a".repeat(40),
        },
        confirmed_at_unix_ms: 100,
        expires_at_unix_ms: 200,
        files: vec![PrivateFileSpecV1 {
            path: ".env.production".into(),
            size_bytes: 5,
            sha256: digest(b"token"),
            data_class: PrivateDataClassV1::CredentialFile,
        }],
    }
}

fn private_chunk(grant_id: &str, path: &str, offset: u64, bytes: &[u8]) -> PrivateFileChunkV1 {
    PrivateFileChunkV1::new(grant_id.into(), path.into(), offset, bytes.to_vec()).unwrap()
}

#[test]
fn grant_is_bound_to_unexpired_exact_protected_file_metadata() {
    let grant = grant();
    grant.validate_at(150).unwrap();
    grant
        .validate_against_exclusions(&[".env.production".into()])
        .unwrap();
    assert!(grant.validate_at(200).is_err());
    assert!(grant.validate_against_exclusions(&[".env".into()]).is_err());

    let mut forged = grant;
    forged.files[0].path = "src/main.rs".into();
    assert!(forged.validate_at(150).is_err());
}

#[test]
fn private_chunks_validate_digest_and_protected_path_without_debug_bytes() {
    let chunk = PrivateFileChunkV1::new(
        "00000000-0000-4000-8000-000000000141".into(),
        ".env.production".into(),
        0,
        b"token".to_vec(),
    )
    .unwrap();
    chunk.validate().unwrap();

    let mut forged = chunk;
    forged.sha256 = "0".repeat(64);
    assert!(forged.validate().is_err());
}

#[test]
fn private_chunk_header_contains_metadata_only_and_rejects_inline_bytes() {
    let chunk = private_chunk(
        "00000000-0000-4000-8000-000000000141",
        ".env.production",
        0,
        b"token",
    );
    let value = serde_json::to_value(chunk.header().unwrap()).unwrap();
    assert_eq!(value["byte_count"], 5);
    assert!(!value.as_object().unwrap().contains_key("bytes"));

    let inline_bytes = serde_json::json!({
        "grant_id": "00000000-0000-4000-8000-000000000141",
        "path": ".env.production",
        "offset": 0,
        "byte_count": 5,
        "sha256": "a".repeat(64),
        "bytes": "dG9rZW4="
    });
    assert!(serde_json::from_value::<PrivateFileChunkHeaderV1>(inline_bytes).is_err());
}

#[test]
fn grants_reject_malformed_identity_paths_and_case_collisions() {
    let mut malformed = grant();
    malformed.grant_id = "not-a-uuid".into();
    assert!(malformed.validate_at(150).is_err());

    let mut malformed = grant();
    malformed.worker_host_key_fingerprint =
        "SHA256:!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!".into();
    assert!(malformed.validate_at(150).is_err());

    let mut malformed = grant();
    malformed.files[0].path = "dir/../.env.production".into();
    assert!(malformed.validate_at(150).is_err());

    let mut git_metadata = grant();
    git_metadata.files[0].path = ".git/config".into();
    assert!(git_metadata.validate_at(150).is_err());

    let mut collision = grant();
    collision.files[0].path = ".ENV.a".into();
    collision.files.push(PrivateFileSpecV1 {
        path: ".env.A".into(),
        size_bytes: 1,
        sha256: "a".repeat(64),
        data_class: PrivateDataClassV1::CredentialFile,
    });
    collision
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    assert!(collision.validate_at(150).is_err());
}

#[test]
fn nested_private_file_is_bound_to_its_excluded_parent_component() {
    let mut grant = grant();
    grant.files[0].path = ".aws/credentials".into();
    assert!(grant.validate_against_exclusions(&[".aws".into()]).is_ok());
    assert!(
        grant
            .validate_against_exclusions(&[".aws-other".into()])
            .is_err()
    );
}

#[test]
fn chunks_must_match_authorized_file_path_hash_identity_and_size() {
    let grant = grant();
    let chunk = PrivateFileChunkV1::new(
        grant.grant_id.clone(),
        ".env.production".into(),
        0,
        b"token".to_vec(),
    )
    .unwrap();
    chunk.validate_for_grant(&grant, 150).unwrap();

    let wrong_path = private_chunk(&grant.grant_id, ".env.other", 0, b"token");
    assert!(wrong_path.validate_for_grant(&grant, 150).is_err());

    let wrong_grant = private_chunk(
        "00000000-0000-4000-8000-000000000147",
        ".env.production",
        0,
        b"token",
    );
    assert!(wrong_grant.validate_for_grant(&grant, 150).is_err());

    let beyond_file = private_chunk(&grant.grant_id, ".env.production", 5, b"token");
    assert!(beyond_file.validate_for_grant(&grant, 150).is_err());
    assert!(beyond_file.validate_for_grant(&grant, 200).is_err());

    grant
        .validate_complete_file(".env.production", b"token")
        .unwrap();
    assert!(
        grant
            .validate_complete_file(".env.production", b"wrong")
            .is_err()
    );
    assert!(
        grant
            .validate_complete_file(".env.other", b"token")
            .is_err()
    );
}
