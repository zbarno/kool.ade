use super::*;

#[test]
fn generic_client_request_rejects_private_bytes_before_transport() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let mut client = RemoteWorkerClient {
        config: crate::execution::remote::ssh::SshWorkerConfig {
            stable_id: "test-worker".into(),
            host_alias: "worker".into(),
            ssh_principal: "builder".into(),
            verified_host_key_fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                .into(),
            known_hosts_path: std::path::PathBuf::new(),
            ssh_binary: std::path::PathBuf::new(),
        },
        transport: Box::new(FakeTransport {
            generation: 1,
            legacy_response: false,
            fail_version_two_health: false,
            sent: Arc::clone(&sent),
            responses: VecDeque::new(),
        }),
        worker_id: "test-worker".into(),
        probe: unavailable_probe(),
        sequences: HashMap::new(),
        last_health_check: Instant::now(),
        system_image_capability_key: zeroize::Zeroizing::new([0x22; 32]),
    };

    let error = client
        .request(
            Kind::PrivateFileChunk,
            "00000000-0000-4000-8000-000000000142",
            serde_json::json!({"chunk":{"bytes":"c3ludGhldGljLXNlY3JldA=="}}),
        )
        .unwrap_err();

    assert_eq!(
        error.kind,
        crate::execution::errors::ExecutionErrorKind::PolicyDenied
    );
    let direct_envelope = EnvelopeV1 {
        version: PROTOCOL_VERSION_V2,
        request_id: "00000000-0000-4000-8000-000000000147".into(),
        execution_id: "00000000-0000-4000-8000-000000000142".into(),
        sequence: 1,
        kind: Kind::PrivateFileChunk,
        payload: serde_json::json!({"token":"synthetic-secret"}),
    };
    assert!(client.send_envelope(&direct_envelope).is_err());
    assert!(sent.lock().unwrap().is_empty());
}

#[test]
fn denied_private_chunk_payload_is_zeroized_before_return() {
    let mut payload = serde_json::json!({
        "chunk": {"bytes": "c3ludGhldGljLXNlY3JldA=="}
    });
    let error = super::deny_private_chunk_payload(&mut payload);
    assert_eq!(
        error.kind,
        crate::execution::errors::ExecutionErrorKind::PolicyDenied
    );
    assert_eq!(payload, serde_json::Value::Null);
    assert!(!format!("{error:?}").contains("secret"));
}
