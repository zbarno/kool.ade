use super::*;
use crate::execution::{
    contract::WorkspaceRef,
    docker::{
        image_extension::{
            LinuxDistribution, PackageTrust, SystemImageExtensionRequest,
            builder::PackageArtifactIdentity,
        },
        images::{ImageLock, RuntimeProfile},
    },
    remote::{
        WorkspaceManifestV1,
        protocol::{EnvelopeKindV1, EnvelopeV1, PROTOCOL_VERSION_V2, read_frame, write_frame},
        staging::{TransferManifestV1, WorkerStager, stream_file_chunks},
    },
};
use sha2::{Digest, Sha256};
use std::{fs, io::Cursor, process::Command, sync::Arc};

const CONTROL_ID: &str = "00000000-0000-4000-8000-000000000000";
const CAPABILITY_KEY: [u8; 32] = [0x5a; 32];
const RECONNECTED_CAPABILITY_KEY: [u8; 32] = [0x3c; 32];

fn encode_key(key: &[u8; 32]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Production boundary: authenticated framed controller/worker RPC dispatches a
/// task-scoped authorization and package stream to the worker backend. This
/// in-memory framing test proves identity/sequence checks, chunk transfer,
/// worker authorization reconstruction and returned image selection data;
/// unit tests of individual validators cannot prove their RPC ordering and
/// serialization agree. It uses a synthetic backend and temporary workspace,
/// with no Docker, SSH process, or network dependency (low setup cost).
#[test]
fn framed_remote_system_image_transfer_derives_and_returns_pinned_digest_after_reconnect() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("README.md"), b"synthetic workspace\n").unwrap();
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
                .env("GIT_TERMINAL_PROMPT", "0")
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

    let bytes = b"synthetic verified Debian package".to_vec();
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
    let transfer_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let decision = crate::harness::DependencyDecision::UserAuthorizeForTask;
    let capability = crate::execution::docker::image_extension::sign_system_image_capability(
        &RECONNECTED_CAPABILITY_KEY,
        &transfer_id,
        &proposal,
        decision,
        &workspace,
        std::slice::from_ref(&identity),
    )
    .unwrap();
    let frames = [
        (
            EnvelopeKindV1::Health,
            serde_json::json!({"wire_version":PROTOCOL_VERSION_V2,"system_image_key":encode_key(&RECONNECTED_CAPABILITY_KEY)}),
        ),
        (
            EnvelopeKindV1::BeginSystemImageTransfer,
            serde_json::json!({
                "transfer_id": transfer_id.clone(),
                "capability": capability.clone(),
                "proposal": proposal,
                "decision": decision,
                "workspace": workspace,
                "manifest": [identity],
            }),
        ),
        (
            EnvelopeKindV1::SystemImageTransferChunk,
            serde_json::json!({
                "transfer_id": transfer_id.clone(),
                "capability": capability.clone(),
                "package_index": 0,
                "offset": 0,
                "bytes": bytes,
            }),
        ),
        (
            EnvelopeKindV1::FinalizeSystemImageTransfer,
            serde_json::json!({"transfer_id": transfer_id,"capability":capability}),
        ),
    ];
    let mut input = Vec::new();
    for (index, (kind, payload)) in frames.into_iter().enumerate() {
        let is_health = kind == EnvelopeKindV1::Health;
        write_frame(
            &mut input,
            &EnvelopeV1 {
                version: PROTOCOL_VERSION_V2,
                request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                execution_id: if is_health {
                    crate::execution::remote::protocol::SYSTEM_IMAGE_SESSION_ID_V1.into()
                } else {
                    CONTROL_ID.into()
                },
                sequence: if is_health { 1 } else { index as u64 },
                kind,
                payload,
            },
        )
        .unwrap();
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let backend: Arc<dyn crate::execution::backend::ExecutionBackend> =
        Arc::new(FakeBackend(Arc::clone(&calls)));
    // Model an SSH loss after the first authenticated session was established.
    // The reconnect must bootstrap a fresh key on the same worker process stream
    // that receives the subsequent transfer frames.
    let mut bootstrap = Vec::new();
    write_frame(&mut bootstrap, &EnvelopeV1 {
        version: PROTOCOL_VERSION_V2,
        request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        execution_id: crate::execution::remote::protocol::SYSTEM_IMAGE_SESSION_ID_V1.into(),
        sequence: 1,
        kind: EnvelopeKindV1::Health,
        payload: serde_json::json!({"wire_version":PROTOCOL_VERSION_V2,"system_image_key":encode_key(&CAPABILITY_KEY)}),
    }).unwrap();
    let mut bootstrap_output = Vec::new();
    serve_stdio_with_journal(
        Arc::clone(&backend),
        Some(stager.clone()),
        None,
        Cursor::new(bootstrap),
        &mut bootstrap_output,
        RequestJournal::new(root.path().join("bootstrap-journal")),
    )
    .unwrap();
    assert_eq!(
        read_frame(&mut Cursor::new(bootstrap_output))
            .unwrap()
            .unwrap()
            .kind,
        EnvelopeKindV1::Acknowledge
    );
    let mut output = Vec::new();
    serve_stdio_with_journal(
        backend,
        Some(stager.clone()),
        None,
        Cursor::new(input),
        &mut output,
        RequestJournal::new(root.path().join("journal")),
    )
    .unwrap();

    let mut output = Cursor::new(output);
    for expected in [
        EnvelopeKindV1::Acknowledge,
        EnvelopeKindV1::Acknowledge,
        EnvelopeKindV1::Acknowledge,
    ] {
        assert_eq!(read_frame(&mut output).unwrap().unwrap().kind, expected);
    }
    let derived = read_frame(&mut output).unwrap().unwrap();
    assert_eq!(derived.kind, EnvelopeKindV1::Acknowledge);
    let image: crate::execution::docker::image_extension::DerivedImage =
        serde_json::from_value(derived.payload).unwrap();
    assert!(image.image_id.starts_with("sha256:"));
    assert_eq!(image.target_platform, "linux/amd64");
    assert_eq!(image.package_manifest[0].name, "curl");
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    assert!(read_frame(&mut output).unwrap().is_none());
}

#[path = "system_image_rpc_negative_tests.rs"]
mod negative_tests;
