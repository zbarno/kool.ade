use super::*;
use crate::execution::{
    errors::{ExecutionError, ExecutionErrorKind},
    session::AuthorizationGenerationVerifier,
};
use crate::harness::{DependencyDecision, DependencyKind, DependencyNeed, PackageEcosystem};
use std::{
    io::{BufRead, BufReader},
    os::unix::net::UnixListener,
    sync::atomic::{AtomicBool, Ordering},
    thread,
};

const TASK: &str = "00000000-0000-4000-8000-000000000001";
const WORKER: &str = "00000000-0000-4000-8000-000000000002";
const EXECUTION: &str = "00000000-0000-4000-8000-000000000003";
const GENERATION: &str = "00000000-0000-4000-8000-000000000004";

#[derive(Debug)]
struct Verifier {
    active: AtomicBool,
    generation: String,
}

impl AuthorizationGenerationVerifier for Verifier {
    fn generation(&self) -> String {
        self.generation.clone()
    }

    fn verify(&self, generation: &str) -> Result<(), ExecutionError> {
        if self.active.load(Ordering::Acquire) && generation == self.generation {
            Ok(())
        } else {
            Err(ExecutionError::new(
                ExecutionErrorKind::PolicyDenied,
                "test authorization is stale",
                false,
            ))
        }
    }
}

fn request() -> DependencyRelayRequestV1 {
    DependencyRelayRequestV1 {
        request_id: "00000000-0000-4000-8000-000000000005".into(),
        worker_id: WORKER.into(),
        execution_id: EXECUTION.into(),
        task_id: TASK.into(),
        workspace: crate::execution::contract::WorkspaceRef {
            project_id: "00000000-0000-4000-8000-000000000006".into(),
            task_id: TASK.into(),
            source_sha: "a".repeat(40),
        },
        authorization_generation: GENERATION.into(),
        cancellation_generation: 1,
        need: DependencyNeed {
            ecosystem: PackageEcosystem::Npm,
            package: Some("archive-helper".into()),
            version: Some("1.0.0".into()),
            source: Some("https://registry.npmjs.org".into()),
            command: "npm ci".into(),
            reason: "Restore approved packages".into(),
            kind: DependencyKind::ExistingRestore,
            lockfile_identity: Some("sha256:fixture".into()),
            introduced_packages: Vec::new(),
        },
    }
}

fn binding(socket: std::path::PathBuf) -> RemoteDependencyBridge {
    RemoteDependencyBridge {
        socket,
        worker_id: WORKER.into(),
        execution_id: EXECUTION.into(),
        task_id: TASK.into(),
        authorization_generation: GENERATION.into(),
        verifier: Arc::new(Verifier {
            active: AtomicBool::new(true),
            generation: GENERATION.into(),
        }),
        cancel: Arc::new(AtomicBool::new(false)),
        stop: Arc::new(AtomicBool::new(false)),
    }
}

fn prepared_response() -> String {
    let dependency = crate::harness::DependencyRequest {
        id: "00000000-0000-4000-8000-000000000007".into(),
        task_id: TASK.into(),
        need: request().need,
        category: crate::harness::DependencyFailureCategory::DependencyRestoreRequired,
        decision: DependencyDecision::AuthorizeForTask,
        rationale: "Approved for this task".into(),
        risk: "Package integrity was verified".into(),
        status: crate::harness::DependencyRequestStatus::Prepared,
        preparation: None,
    };
    serde_json::json!({
        "status": "prepared",
        "summary": "Dependency cache is ready.",
        "dependency_request": dependency
    })
    .to_string()
}

#[test]
fn relay_binding_rejects_task_mismatch_and_stale_generation() {
    assert!(!broker_task_matches(
        None,
        "00000000-0000-4000-8000-000000000001"
    ));
    assert!(!broker_task_matches(
        Some("00000000-0000-4000-8000-000000000002"),
        "00000000-0000-4000-8000-000000000001"
    ));
    let verifier = Arc::new(Verifier {
        active: AtomicBool::new(true),
        generation: "00000000-0000-4000-8000-000000000001".into(),
    });
    assert!(
        verifier
            .verify("00000000-0000-4000-8000-000000000002")
            .is_err()
    );
    verifier.active.store(false, Ordering::Release);
    assert!(
        verifier
            .verify("00000000-0000-4000-8000-000000000001")
            .is_err()
    );
}

#[test]
fn stale_or_cancelled_binding_interrupts_response_wait() {
    let verifier = Arc::new(Verifier {
        active: AtomicBool::new(false),
        generation: "00000000-0000-4000-8000-000000000001".into(),
    });
    let binding = RemoteDependencyBridge {
        socket: std::path::PathBuf::new(),
        worker_id: String::new(),
        execution_id: String::new(),
        task_id: String::new(),
        authorization_generation: verifier.generation(),
        verifier,
        cancel: Arc::new(AtomicBool::new(false)),
        stop: Arc::new(AtomicBool::new(false)),
    };
    let (mut reader, _writer) = UnixStream::pair().unwrap();
    reader
        .set_read_timeout(Some(Duration::from_millis(1)))
        .unwrap();
    assert!(
        read_bounded_response_until(
            &mut reader,
            &binding,
            Instant::now() + Duration::from_millis(10)
        )
        .is_err()
    );
    binding.cancel.store(true, Ordering::Release);
    assert!(binding.verify_active().is_err());
}

#[test]
fn broker_denials_never_become_prepared_results() {
    assert_eq!(relay_outcome("rejected"), DependencyRelayOutcomeV1::Denied);
    assert_eq!(
        relay_outcome("needs_attention"),
        DependencyRelayOutcomeV1::AwaitingManager
    );
    assert_eq!(relay_outcome("error"), DependencyRelayOutcomeV1::Failed);
}

#[test]
fn remote_prepare_relays_broker_result_for_the_bound_execution() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("resource.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let responder = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut line = Vec::new();
        BufReader::new(stream.try_clone().unwrap())
            .read_until(b'\n', &mut line)
            .unwrap();
        let request: serde_json::Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(request["action"], "dependency_request");
        stream
            .try_clone()
            .unwrap()
            .write_all(format!("{}\n", prepared_response()).as_bytes())
            .unwrap();
    });
    let result = binding(socket).prepare(request()).unwrap();
    responder.join().unwrap();
    assert_eq!(
        result.relay_response.outcome,
        DependencyRelayOutcomeV1::Prepared
    );
    assert_eq!(
        result.relay_response.dependency_request_id.as_deref(),
        Some("00000000-0000-4000-8000-000000000007")
    );
    assert_eq!(result.broker_response["status"], "prepared");
}

#[test]
fn remote_prepare_rejects_wrong_task_or_execution_before_socket_io() {
    let bridge = binding(std::path::PathBuf::new());
    let mut wrong_task = request();
    wrong_task.task_id = "00000000-0000-4000-8000-000000000008".into();
    wrong_task.workspace.task_id = wrong_task.task_id.clone();
    assert!(bridge.prepare(wrong_task).is_err());

    let mut wrong_execution = request();
    wrong_execution.execution_id = "00000000-0000-4000-8000-000000000009".into();
    assert!(bridge.prepare(wrong_execution).is_err());
}

#[test]
fn remote_prepare_rejects_unavailable_broker_and_times_out_waiting_for_response() {
    let directory = tempfile::tempdir().unwrap();
    let bridge = binding(directory.path().join("missing.sock"));
    assert!(bridge.prepare(request()).is_err());

    let (mut reader, _writer) = UnixStream::pair().unwrap();
    reader
        .set_read_timeout(Some(Duration::from_millis(1)))
        .unwrap();
    assert!(
        read_bounded_response_until(
            &mut reader,
            &binding(std::path::PathBuf::new()),
            Instant::now() + Duration::from_millis(10)
        )
        .is_err()
    );
}

#[test]
fn remote_prepare_rejects_responses_over_the_broker_limit() {
    let binding = binding(std::path::PathBuf::new());
    let (mut reader, mut writer) = UnixStream::pair().unwrap();
    reader
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let responder = thread::spawn(move || {
        let oversized = vec![b'x'; MAX_RESPONSE_BYTES + 1];
        writer.write_all(&oversized).unwrap();
    });
    assert!(
        read_bounded_response_until(
            &mut reader,
            &binding,
            Instant::now() + Duration::from_secs(1)
        )
        .is_err()
    );
    responder.join().unwrap();
}
