use super::*;

#[test]
fn remote_package_stream_rejects_replayed_offsets_and_unknown_package_indexes() {
    let transfer_id = "00000000-0000-4000-8000-000000000001";
    let identity = crate::execution::docker::image_extension::builder::PackageArtifactIdentity {
        name: "curl".into(),
        version: "1.0-1".into(),
        architecture: "amd64".into(),
        source: "https://deb.debian.org/debian/pool/main/c/curl/curl_1.0-1_amd64.deb".into(),
        sha256: "a".repeat(64),
    };
    let proposal = crate::execution::docker::image_extension::SystemImageExtensionRequest {
        task_id: "00000000-0000-4000-8000-000000000002".into(),
        project_id: "00000000-0000-4000-8000-000000000003".into(),
        distribution: crate::execution::docker::image_extension::LinuxDistribution::DebianBookworm,
        base_profile: crate::execution::docker::images::RuntimeProfile::General,
        base_image_digest: format!("sha256:{}", "b".repeat(64)),
        target_arch: "amd64".into(),
        packages: vec!["curl".into()],
        trust:
            crate::execution::docker::image_extension::PackageTrust::ApprovedDistributionRepository,
        purpose: "Install approved tool for task".into(),
    };
    let mut transfers = SystemImageTransfers::default();
    transfers.0.insert(
        transfer_id.into(),
        Pending {
            capability: "test-capability".into(),
            proposal,
            decision: crate::harness::DependencyDecision::UserAuthorizeForTask,
            workspace: crate::execution::contract::WorkspaceRef {
                project_id: "00000000-0000-4000-8000-000000000003".into(),
                task_id: "00000000-0000-4000-8000-000000000002".into(),
                source_sha: "a".repeat(40),
            },
            identities: vec![identity],
            bytes: vec![Vec::new()],
            total_bytes: 0,
        },
    );
    let envelope = |sequence| EnvelopeV1 {
        version: crate::execution::remote::protocol::PROTOCOL_VERSION_V1,
        request_id: format!("00000000-0000-4000-8000-{sequence:012}"),
        execution_id: "00000000-0000-4000-8000-000000000000".into(),
        sequence,
        kind: Kind::SystemImageTransferChunk,
        payload: Value::Null,
    };
    let wrong_offset = serde_json::json!({"transfer_id":transfer_id,"capability":"test-capability","package_index":0,"offset":1,"bytes":[1]});
    assert!(chunk(&wrong_offset, &mut transfers, &envelope(1)).is_err());
    let unknown_package = serde_json::json!({"transfer_id":transfer_id,"capability":"test-capability","package_index":1,"offset":0,"bytes":[1]});
    assert!(chunk(&unknown_package, &mut transfers, &envelope(2)).is_err());
    let valid = serde_json::json!({"transfer_id":transfer_id,"capability":"test-capability","package_index":0,"offset":0,"bytes":[1,2,3]});
    assert!(chunk(&valid, &mut transfers, &envelope(3)).is_ok());
    assert_eq!(transfers.0[transfer_id].bytes[0], [1, 2, 3]);
}
