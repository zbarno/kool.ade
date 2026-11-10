use super::*;

#[test]
fn artifact_bridge_round_trips_through_the_fenced_remote_worker_stream() {
    let root = tempfile::tempdir().unwrap();
    let key = signing_key();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let session = signed_session(&key, now);
    let config = host_config(&key);
    let ssh_identity = VerifiedComputerRemoteWorkerIdentityV1::from_host_config(&config);
    let store = SharedCheckpoint::default();
    let guest = Arc::new(Mutex::new(FakeGuest::default()));
    guest
        .lock()
        .unwrap()
        .files
        .insert("report.txt".into(), b"worker result".to_vec());

    let (controller_stream, worker_stream) = UnixStream::pair().unwrap();
    let worker_input = worker_stream.try_clone().unwrap();
    let worker_config = config.clone();
    let worker_store = store.clone();
    let worker_guest = guest.clone();
    let server = std::thread::spawn(move || {
        serve_computer_remote_worker_stdio_v1(
            worker_input,
            worker_stream,
            worker_store,
            FakeRuntime {
                config: worker_config.clone(),
                guest: worker_guest,
            },
            &worker_config,
        )
    });
    let mut client =
        ComputerRemoteWorkerClientV1::from_test_stream(controller_stream, ssh_identity, 1);

    let start = request(
        &session,
        1,
        Body::Start {
            session: session.clone(),
        },
    );
    let started = client.request(&start).unwrap();
    let ResultValue::Prepared { handle, .. } = started.result else {
        panic!("worker should return a prepared remote VM");
    };
    let boot = request(
        &session,
        2,
        Body::Operation {
            operation: Operation::Boot {
                handle: handle.clone(),
            },
        },
    );
    assert!(matches!(
        client.request(&boot).unwrap().result,
        ResultValue::BackendEvent(_)
    ));
    let display = request(
        &session,
        3,
        Body::Operation {
            operation: Operation::CaptureDisplay {
                handle: handle.clone(),
            },
        },
    );
    assert!(matches!(
        client.request(&display).unwrap().result,
        ResultValue::DisplayFrame(_)
    ));
    let action = ComputerActionV1 {
        schema_version: 1,
        session_id: session.identity.session_id.clone(),
        action_sequence: 1,
        idempotency_key: uuid::Uuid::new_v4().to_string(),
        actor: ComputerActionActorV1::Agent,
        lease_generation: session.lease.generation,
        approval_generation: session.approval_generation,
        cancellation_generation: session.cancellation_generation,
        operation: ComputerOperationV1::ObserveScreen,
        payload: ComputerActionPayloadV1::ObserveScreen,
    };
    let action_request = request(&session, 4, Body::Action { action });
    assert!(matches!(
        client.request(&action_request).unwrap().result,
        ResultValue::GuestAction(_)
    ));

    let backend = RemoteComputerBackendV1 {
        enrollment: controller_enrollment(),
        ssh: controller_enrollment().ssh_config().unwrap(),
        controller_key_id: CONTROLLER_KEY_ID.into(),
        controller_key: key.verifying_key(),
        journal_root: root.path().join("journals"),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        recovery_store: ComputerRemoteWorkerSessionStoreV1::open(
            root.path().join("controller-recovery"),
        )
        .unwrap(),
    };
    backend
        .recovery_store
        .save(&ComputerRemoteWorkerRecoveryRecordV1 {
            schema_version: 1,
            worker_id: WORKER_ID.into(),
            machine_id: MACHINE_ID.into(),
            session: session.clone(),
            last_transport_sequence: 4,
            last_event_sequence: 2,
            pending_action_sequence: None,
            outcome_unknown: false,
        })
        .unwrap();
    backend.sessions.lock().unwrap().insert(
        session.identity.session_id.clone(),
        ActiveRemoteSession {
            session: session.clone(),
            handle,
            client,
            last_event_sequence: 2,
        },
    );

    let planning = crate::artifacts::planning_store::PlanningStore::new(
        uuid::Uuid::new_v4(),
        root.path().join("planning-store"),
        crate::artifacts::planning_store::StoreMode::ManagedLocal,
    );
    let bridge = crate::computer_use::artifacts::ComputerArtifactBridgeV1::new(planning);
    let task_root = root.path().join("task");
    std::fs::create_dir_all(task_root.join("src")).unwrap();
    std::fs::write(task_root.join("src/input.txt"), b"task input").unwrap();
    let identity = session.identity.clone();
    let input_grant = bridge
        .issue_grant(ComputerTransferGrantRequestV1 {
            identity: identity.clone(),
            flow: ComputerArtifactFlowV1::TaskArtifactToGuest,
            task_revision: REVISION.into(),
            relative_path: "src/input.txt".into(),
            max_bytes: 1024,
            now_unix_ms: now,
            expires_at_unix_ms: now + 50_000,
            lease_generation: session.lease.generation,
            approval_generation: session.approval_generation,
            user_approved: false,
        })
        .unwrap();
    bridge
        .stage_task_file_to_remote_guest(
            &backend,
            &input_grant.grant_id,
            &task_root,
            REVISION,
            &session,
            now + 1,
        )
        .unwrap();
    assert_eq!(guest.lock().unwrap().files["input.txt"], b"task input");

    let output_grant = bridge
        .issue_grant(ComputerTransferGrantRequestV1 {
            identity,
            flow: ComputerArtifactFlowV1::GuestArtifactToTask,
            task_revision: REVISION.into(),
            relative_path: "outputs/report.txt".into(),
            max_bytes: 1024,
            now_unix_ms: now,
            expires_at_unix_ms: now + 50_000,
            lease_generation: session.lease.generation,
            approval_generation: session.approval_generation,
            user_approved: false,
        })
        .unwrap();
    let receipt = bridge
        .receive_remote_guest_file(
            &backend,
            &output_grant.grant_id,
            &session,
            REVISION,
            "report.txt",
            now + 2,
        )
        .unwrap();
    let (_, downloaded) = bridge
        .load_for_task_apply(
            &receipt.artifact_id,
            &session.identity,
            REVISION,
            session.lease.generation,
            session.approval_generation,
            now + 3,
        )
        .unwrap();
    assert_eq!(downloaded, b"worker result");
    assert_eq!(
        guest.lock().unwrap().operations,
        [
            "start",
            "boot",
            "display",
            "action",
            "artifact_write",
            "artifact_read"
        ]
    );

    drop(backend);
    assert!(server.join().unwrap().is_ok());
}
