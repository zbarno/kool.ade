use super::*;

/// Production boundary: framed worker RPC rejects unauthorized or replayed
/// image transfer messages before backend derivation. Unit validator tests do
/// not prove request ordering and serialized authorization agree at this route.
/// Uses synthetic packages/workspace and an in-memory backend; no Docker, SSH,
/// or network dependency (low setup cost).
#[test]
fn framed_remote_system_image_rpc_rejects_forgery_wrong_task_decision_and_replay() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("README.md"), b"synthetic workspace\\n").unwrap();
    for args in [
        vec!["init"],
        vec!["config", "user.name", "Synthetic Test"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["add", "README.md"],
        vec!["commit", "-m", "synthetic workspace"],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&source)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    let source_sha = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&source)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    let workspace = WorkspaceRef {
        project_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        task_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        source_sha,
    };
    let snapshot = WorkspaceManifestV1::capture(workspace.clone(), &source).unwrap();
    let transfer =
        TransferManifestV1::workspace("controller:fixture", "worker:fixture", snapshot).unwrap();
    let stager = Arc::new(WorkerStager::new(root.path(), "fixture".into()).unwrap());
    stager.begin(transfer.clone()).unwrap();
    for (path, _) in transfer.file_specs() {
        stream_file_chunks(&transfer, &source, path, |chunk| stager.accept_chunk(chunk)).unwrap();
    }
    stager.finalize(&transfer.transfer_id).unwrap();

    let bytes = b"original verified package bytes".to_vec();
    let identity = PackageArtifactIdentity {
        name: "curl".into(),
        version: "7.88.1-10+deb12u14".into(),
        architecture: "amd64".into(),
        source: "https://deb.debian.org/debian/pool/main/c/curl/curl_7.88.1-10+deb12u14_amd64.deb"
            .into(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    };
    let base = ImageLock::bundled()
        .unwrap()
        .select(RuntimeProfile::General, "x86_64")
        .unwrap();
    let proposal = SystemImageExtensionRequest {
        task_id: workspace.task_id.clone(),
        project_id: workspace.project_id.clone(),
        distribution: LinuxDistribution::DebianBookworm,
        base_profile: RuntimeProfile::General,
        base_image_digest: base.digest,
        target_arch: "x86_64".into(),
        packages: vec!["curl".into()],
        trust: PackageTrust::ApprovedDistributionRepository,
        purpose: "Provide the approved HTTP client required by this task".into(),
    };
    let valid_transfer = uuid::Uuid::new_v4().hyphenated().to_string();
    let decision = crate::harness::DependencyDecision::UserAuthorizeForTask;
    let sign = |id: &str, proposal: &SystemImageExtensionRequest, decision| {
        crate::execution::docker::image_extension::sign_system_image_capability(
            &CAPABILITY_KEY,
            id,
            proposal,
            decision,
            &workspace,
            std::slice::from_ref(&identity),
        )
        .unwrap()
    };
    let valid_capability = sign(&valid_transfer, &proposal, decision);
    let mut wrong_task = proposal.clone();
    wrong_task.task_id = "00000000-0000-4000-8000-000000000999".into();
    let wrong_task_capability = sign(&valid_transfer, &proposal, decision);
    let rejected = uuid::Uuid::new_v4().hyphenated().to_string();
    let wrong_decision = crate::harness::DependencyDecision::AuthorizeForTask;
    let wrong_decision_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let forged_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let replay_begin = serde_json::json!({"transfer_id":valid_transfer,"capability":valid_capability,
        "proposal":proposal,"decision":decision,"workspace":workspace,"manifest":[identity]});
    let frames = vec![
        (
            EnvelopeKindV1::Health,
            serde_json::json!({"wire_version":PROTOCOL_VERSION_V2,"system_image_key":encode_key(&CAPABILITY_KEY)}),
        ),
        (
            EnvelopeKindV1::BeginSystemImageTransfer,
            serde_json::json!({"transfer_id":rejected,"capability":wrong_task_capability,
            "proposal":wrong_task,"decision":decision,"workspace":workspace,"manifest":[identity]}),
        ),
        (
            EnvelopeKindV1::BeginSystemImageTransfer,
            serde_json::json!({"transfer_id":wrong_decision_id,
            "capability":sign(&wrong_decision_id,&proposal,wrong_decision),"proposal":proposal,"decision":wrong_decision,
            "workspace":workspace,"manifest":[identity]}),
        ),
        (
            EnvelopeKindV1::BeginSystemImageTransfer,
            serde_json::json!({"transfer_id":forged_id,"capability":"hmac-sha256:00",
            "proposal":proposal,"decision":decision,"workspace":workspace,"manifest":[identity]}),
        ),
        (
            EnvelopeKindV1::BeginSystemImageTransfer,
            replay_begin.clone(),
        ),
        (
            EnvelopeKindV1::SystemImageTransferChunk,
            serde_json::json!({"transfer_id":valid_transfer,"capability":"forged",
            "package_index":0,"offset":0,"bytes":bytes}),
        ),
        (
            EnvelopeKindV1::SystemImageTransferChunk,
            serde_json::json!({"transfer_id":valid_transfer,"capability":valid_capability,
            "package_index":0,"offset":0,"bytes":b"tampered package bytes".to_vec()}),
        ),
        (
            EnvelopeKindV1::FinalizeSystemImageTransfer,
            serde_json::json!({"transfer_id":valid_transfer,"capability":"forged"}),
        ),
        (
            EnvelopeKindV1::FinalizeSystemImageTransfer,
            serde_json::json!({"transfer_id":valid_transfer,"capability":valid_capability}),
        ),
        (EnvelopeKindV1::BeginSystemImageTransfer, replay_begin),
        (
            EnvelopeKindV1::FinalizeSystemImageTransfer,
            serde_json::json!({"transfer_id":uuid::Uuid::new_v4().hyphenated().to_string(),"capability":"unknown"}),
        ),
    ];
    let mut input = Vec::new();
    for (index, (kind, payload)) in frames.into_iter().enumerate() {
        write_frame(
            &mut input,
            &EnvelopeV1 {
                version: PROTOCOL_VERSION_V2,
                request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                execution_id: CONTROL_ID.into(),
                sequence: index as u64 + 1,
                kind,
                payload,
            },
        )
        .unwrap();
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let mut output = Vec::new();
    serve_stdio_with_journal(
        Arc::new(FakeBackend(Arc::clone(&calls))),
        Some(stager),
        None,
        Cursor::new(input),
        &mut output,
        RequestJournal::new(root.path().join("journal")),
    )
    .unwrap();
    let mut output = Cursor::new(output);
    let kinds = (0..11)
        .map(|_| read_frame(&mut output).unwrap().unwrap().kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [
            EnvelopeKindV1::Acknowledge,
            EnvelopeKindV1::Error,
            EnvelopeKindV1::Error,
            EnvelopeKindV1::Error,
            EnvelopeKindV1::Acknowledge,
            EnvelopeKindV1::Error,
            EnvelopeKindV1::Acknowledge,
            EnvelopeKindV1::Error,
            EnvelopeKindV1::Error,
            EnvelopeKindV1::Error,
            EnvelopeKindV1::Error
        ]
    );
    assert_eq!(calls.load(Ordering::Relaxed), 2);
}
