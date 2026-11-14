use super::*;

pub(super) fn worker_config(
    key: &SigningKey,
    worker_id: &str,
    machine_id: &str,
    host_key: char,
    attestation_seed: u8,
    attestation_id: &str,
) -> ComputerRemoteWorkerHostConfigV1 {
    let mut config = host_config(key);
    config.worker_id = worker_id.into();
    config.machine_id = machine_id.into();
    config.ssh_host_key_fingerprint = format!("SHA256:{}", host_key.to_string().repeat(43));
    config.worker_attestation_key_id = attestation_id.into();
    config.worker_attestation_private_key_hex = hex(&[attestation_seed; 32]);
    config
}

pub(super) fn session_for_worker(
    key: &SigningKey,
    config: &ComputerRemoteWorkerHostConfigV1,
    attestation_seed: u8,
) -> ComputerSessionV1 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut session = signed_session(key, now);
    let attestation_key = SigningKey::from_bytes(&[attestation_seed; 32]);
    session.target = ComputerTargetV1::RemoteVmWorker {
        worker_id: config.worker_id.clone(),
        machine_id: config.machine_id.clone(),
        trust: crate::computer_use::ComputerRemoteWorkerTrustV1 {
            ssh_host_key_fingerprint: config.ssh_host_key_fingerprint.clone(),
            worker_attestation_key_id: config.worker_attestation_key_id.clone(),
            worker_attestation_verifying_key_hex: hex(attestation_key.verifying_key().as_bytes()),
            transport_protocol_version:
                crate::computer_use::COMPUTER_USE_REMOTE_WORKER_PROTOCOL_VERSION_V1,
            guest_protocol_version: crate::computer_use::guest_protocol::GUEST_PROTOCOL_VERSION_V1,
        },
    };
    session.attest(CONTROLLER_KEY_ID.into(), key).unwrap();
    session
}

fn start_worker_fixture(
    config: ComputerRemoteWorkerHostConfigV1,
) -> (
    ComputerRemoteWorkerClientV1,
    std::thread::JoinHandle<io::Result<()>>,
    Arc<Mutex<FakeGuest>>,
) {
    let identity = VerifiedComputerRemoteWorkerIdentityV1::from_host_config(&config);
    let guest = Arc::new(Mutex::new(FakeGuest::default()));
    let (controller_stream, worker_stream) = UnixStream::pair().unwrap();
    let worker_input = worker_stream.try_clone().unwrap();
    let worker_config = config.clone();
    let worker_guest = guest.clone();
    let server = std::thread::spawn(move || {
        serve_computer_remote_worker_stdio_v1(
            worker_input,
            worker_stream,
            SharedCheckpoint::default(),
            FakeRuntime {
                config: worker_config.clone(),
                guest: worker_guest,
            },
            &worker_config,
        )
    });
    (
        ComputerRemoteWorkerClientV1::from_test_stream(controller_stream, identity, 1),
        server,
        guest,
    )
}

pub(super) fn worker_request(
    session: &ComputerSessionV1,
    worker_id: &str,
    host_key: &str,
    sequence: u64,
    body: Body,
) -> Request {
    let mut request = request(session, sequence, body);
    request.worker_id = worker_id.into();
    request.ssh_host_key_fingerprint = host_key.into();
    request
}

/// Production boundary: controller client -> independently configured worker
/// stdio servers -> host-specific session fence and VM runtime fixture. This
/// covers worker-stream framing, per-host identity, and session isolation;
/// unit tests of the fence alone cannot prove two live endpoints stay distinct.
/// External dependencies: none (two local worker fixture threads and Unix
/// streams). Expected setup cost: under one second.
#[test]
fn independently_hosted_worker_fixtures_keep_identity_and_sessions_separate() {
    let key = signing_key();
    let east_config = worker_config(
        &key,
        "worker-east",
        "00000000-0000-4000-8000-000000000201",
        'A',
        11,
        "00000000-0000-4000-8000-000000000211",
    );
    let west_config = worker_config(
        &key,
        "worker-west",
        "00000000-0000-4000-8000-000000000202",
        'B',
        12,
        "00000000-0000-4000-8000-000000000212",
    );
    let east_session = session_for_worker(&key, &east_config, 11);
    let west_session = session_for_worker(&key, &west_config, 12);
    let (mut east_probe, east_probe_server, _) = start_worker_fixture(east_config.clone());
    let (mut west_probe, west_probe_server, _) = start_worker_fixture(west_config.clone());

    for (client, config, session) in [
        (&mut east_probe, &east_config, &east_session),
        (&mut west_probe, &west_config, &west_session),
    ] {
        let probe = worker_request(
            session,
            &config.worker_id,
            &config.ssh_host_key_fingerprint,
            1,
            Body::Probe,
        );
        let response = client.request(&probe).unwrap();
        let ResultValue::Advertisement(advertisement) = response.result else {
            panic!("worker should return its own signed advertisement");
        };
        assert_eq!(advertisement.worker_id, config.worker_id);
        assert_eq!(
            advertisement.ssh_host_key_fingerprint,
            config.ssh_host_key_fingerprint
        );
        assert!(advertisement.validate().is_ok());
    }
    drop(east_probe);
    drop(west_probe);
    assert!(east_probe_server.join().unwrap().is_ok());
    assert!(west_probe_server.join().unwrap().is_ok());

    // The production controller opens a new SSH stdio stream after probing.
    let (mut east_client, east_server, east_guest) = start_worker_fixture(east_config.clone());
    let (mut west_client, west_server, west_guest) = start_worker_fixture(west_config.clone());

    let east_start = worker_request(
        &east_session,
        &east_config.worker_id,
        &east_config.ssh_host_key_fingerprint,
        1,
        Body::Start {
            session: east_session.clone(),
        },
    );
    let west_start = worker_request(
        &west_session,
        &west_config.worker_id,
        &west_config.ssh_host_key_fingerprint,
        1,
        Body::Start {
            session: west_session.clone(),
        },
    );
    assert!(matches!(
        east_client.request(&east_start).unwrap().result,
        ResultValue::Prepared { .. }
    ));
    assert!(matches!(
        west_client.request(&west_start).unwrap().result,
        ResultValue::Prepared { .. }
    ));
    assert_eq!(east_guest.lock().unwrap().operations, ["start"]);
    assert_eq!(west_guest.lock().unwrap().operations, ["start"]);

    let wrong_host_request = worker_request(
        &east_session,
        &east_config.worker_id,
        &west_config.ssh_host_key_fingerprint,
        2,
        Body::Operation {
            operation: Operation::Boot {
                handle: ComputerMachineHandleV1::from_remote(
                    east_session.identity.session_id.clone(),
                    east_session.lease.lease_id.clone(),
                    east_session.lease.generation,
                    east_config.machine_id.clone(),
                ),
            },
        },
    );
    assert!(east_client.request(&wrong_host_request).is_err());
    assert_eq!(east_guest.lock().unwrap().operations, ["start"]);

    drop(east_client);
    drop(west_client);
    assert!(east_server.join().unwrap().is_ok());
    assert!(west_server.join().unwrap().is_ok());
}

#[test]
fn worker_fixture_rejects_another_hosts_signed_session_before_starting() {
    let key = signing_key();
    let east_config = worker_config(
        &key,
        "worker-east",
        "00000000-0000-4000-8000-000000000201",
        'A',
        11,
        "00000000-0000-4000-8000-000000000211",
    );
    let west_config = worker_config(
        &key,
        "worker-west",
        "00000000-0000-4000-8000-000000000202",
        'B',
        12,
        "00000000-0000-4000-8000-000000000212",
    );
    let (mut west_client, west_server, west_guest) = start_worker_fixture(west_config.clone());
    let east_session = session_for_worker(&key, &east_config, 11);
    let request = worker_request(
        &east_session,
        &west_config.worker_id,
        &west_config.ssh_host_key_fingerprint,
        1,
        Body::Start {
            session: east_session.clone(),
        },
    );

    assert!(west_client.request(&request).is_err());
    assert!(west_guest.lock().unwrap().operations.is_empty());
    drop(west_client);
    assert!(west_server.join().unwrap().is_err());
}
