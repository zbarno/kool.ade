use super::*;

#[test]
fn version_one_worker_health_response_is_classified_as_incompatible() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let mut client = RemoteWorkerClient {
        config: crate::execution::remote::ssh::SshWorkerConfig {
            stable_id: "test-worker-legacy".into(),
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
        worker_id: "test-worker-legacy".into(),
        probe: unavailable_probe(),
        sequences: HashMap::new(),
        last_health_check: Instant::now(),
        system_image_capability_key: zeroize::Zeroizing::new([0x22; 32]),
    };

    let error = client
        .reconnect_and_negotiate_with(
            || {
                Ok(Box::new(FakeTransport {
                    generation: 2,
                    legacy_response: true,
                    fail_version_two_health: false,
                    sent: Arc::clone(&sent),
                    responses: VecDeque::new(),
                }))
            },
            false,
        )
        .unwrap_err();

    assert_eq!(
        error.kind,
        crate::execution::errors::ExecutionErrorKind::WorkerIncompatible
    );
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].1.version,
        crate::execution::remote::protocol::PROTOCOL_VERSION_V1
    );
    assert_eq!(sent[0].1.kind, Kind::Health);
    assert_eq!(sent[0].1.payload["wire_version"], PROTOCOL_VERSION_V2);
}

/// Production boundary: the exchange layer must preserve the incompatible
/// classification through the reconnect path instead of rewriting it as
/// disconnected. Fake transports exercise both protocol responses without SSH,
/// Docker, or network dependencies (low setup cost).
#[test]
fn failed_v2_health_exchange_keeps_the_legacy_worker_incompatible_result() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let mut client = RemoteWorkerClient {
        config: crate::execution::remote::ssh::SshWorkerConfig {
            stable_id: "test-worker-legacy-exchange".into(),
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
            fail_version_two_health: true,
            sent: Arc::clone(&sent),
            responses: VecDeque::new(),
        }),
        worker_id: "test-worker-legacy-exchange".into(),
        probe: unavailable_probe(),
        sequences: HashMap::new(),
        last_health_check: Instant::now(),
        system_image_capability_key: zeroize::Zeroizing::new([0x22; 32]),
    };

    let error = client
        .exchange_with_reconnect(
            Kind::Health,
            SYSTEM_IMAGE_SESSION_ID_V1,
            serde_json::json!({
                "wire_version": PROTOCOL_VERSION_V2,
                "system_image_key":"2222222222222222222222222222222222222222222222222222222222222222"
            }),
            |client| {
                client.reconnect_and_negotiate_with(
                    || {
                        Ok(Box::new(FakeTransport {
                            generation: 2,
                            legacy_response: true,
                            fail_version_two_health: false,
                            sent: Arc::clone(&sent),
                            responses: VecDeque::new(),
                        }))
                    },
                    false,
                )
            },
        )
        .unwrap_err();

    assert_eq!(
        error.kind,
        crate::execution::errors::ExecutionErrorKind::WorkerIncompatible
    );
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].1.version, PROTOCOL_VERSION_V2);
    assert_eq!(
        sent[1].1.version,
        crate::execution::remote::protocol::PROTOCOL_VERSION_V1
    );
}
