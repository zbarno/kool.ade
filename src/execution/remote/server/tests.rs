use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::execution::remote::protocol;
use crate::execution::{
    backend::{
        BackendCapabilitiesV1, BackendFuture, BackendProbeV1, ExecutionBackend, ExecutionHandleV1,
        WorkspaceLeaseV1,
    },
    contract::{
        EventPageRequestV1, ExecutionEventV1, ExecutionModeV1, ExecutionRequestV1,
        ExecutionResultV1, ExecutionTarget, ResourceBudgetV1, WorkspaceRef,
    },
    policy::ExecutionPolicyV1,
};
use std::collections::BTreeSet;

#[path = "system_image_rpc_tests.rs"]
mod system_image_rpc_tests;

struct FakeBackend(Arc<AtomicUsize>);

impl ExecutionBackend for FakeBackend {
    fn derive_authorized_system_image<'a>(
        &'a self,
        request: &'a crate::execution::docker::image_extension::AuthorizedImageExtensionRequest,
        packages: Vec<crate::execution::docker::image_extension::builder::VerifiedPackageArtifact>,
    ) -> BackendFuture<'a, crate::execution::docker::image_extension::DerivedImage> {
        use crate::execution::docker::image_extension::{DerivedImage, builder::generate_context};
        use sha2::{Digest, Sha256};

        self.0.fetch_add(1, Ordering::Relaxed);
        let result = generate_context(request, packages)
            .map(|context| DerivedImage {
                image_id: format!(
                    "sha256:{:x}",
                    Sha256::digest(context.content_identity.as_bytes())
                ),
                content_identity: context.content_identity,
                base_digest: request.base_digest.clone(),
                target_platform: request.target_platform.clone(),
                package_manifest: context.package_manifest,
            })
            .map_err(|error| {
                ExecutionError::new(
                    crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                    error.to_string(),
                    false,
                )
            });
        ready(result)
    }

    fn probe(&self, _: &ExecutionTarget) -> BackendFuture<'_, BackendProbeV1> {
        self.0.fetch_add(1, Ordering::Relaxed);
        ready(Ok(BackendProbeV1 {
            wire_version: 1,
            available: true,
            compatible: true,
            architecture: Some("amd64".into()),
            failure: None,
            capabilities: Some(BackendCapabilitiesV1 {
                app_version: "0.1.0".into(),
                docker_version: Some("27.0".into()),
                docker_api_version: Some("1.53".into()),
                docker_rootless: Some(true),
                docker_desktop: Some(false),
                linux_containers: Some(true),
                seccomp_enabled: Some(true),
                memory_limit: Some(true),
                cpu_quota: Some(true),
                pids_limit: Some(true),
                disk_available_bytes: Some(10_000_000_000),
                gpu_available: false,
                display_available: false,
                current_execution_count: 0,
                maximum_execution_count: None,
                resource_ceiling: crate::execution::contract::ResourceBudgetV1 {
                    cpu_millis: 16_000,
                    memory_bytes: 64 * 1024 * 1024 * 1024,
                    process_count: 4_096,
                    temporary_disk_bytes: 100 * 1024 * 1024 * 1024,
                    output_bytes: 1024 * 1024 * 1024,
                },
                trusted_image_digests: vec!["sha256:approved".into()],
                policy_hash: "sha256:policy".into(),
            }),
        }))
    }

    fn stage_dependency_cache<'a>(
        &'a self,
        snapshot: crate::execution::backend::DependencyCacheSnapshot,
    ) -> BackendFuture<'a, ()> {
        let snapshot = snapshot.snapshot;
        self.0.fetch_add(1, Ordering::Relaxed);
        ready(
            snapshot
                .manifest
                .validate_ref(&snapshot.cache_ref, &snapshot.archive)
                .map_err(|_| {
                    ExecutionError::new(
                        crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                        "cache rejected",
                        false,
                    )
                }),
        )
    }

    fn prepare_workspace<'a>(
        &'a self,
        _: &'a WorkspaceRef,
        _: &'a ExecutionTarget,
    ) -> BackendFuture<'a, WorkspaceLeaseV1> {
        unavailable()
    }

    fn start<'a>(
        &'a self,
        _: &'a WorkspaceLeaseV1,
        _: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ExecutionHandleV1> {
        self.0.fetch_add(1, Ordering::Relaxed);
        unavailable()
    }

    fn release_workspace<'a>(&'a self, _: &'a WorkspaceLeaseV1) -> BackendFuture<'a, ()> {
        unavailable()
    }

    fn exec<'a>(
        &'a self,
        _: &'a ExecutionHandleV1,
        _: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()> {
        unavailable()
    }

    fn events<'a>(
        &'a self,
        _: &'a ExecutionHandleV1,
        _: EventPageRequestV1,
    ) -> BackendFuture<'a, Vec<ExecutionEventV1>> {
        unavailable()
    }

    fn cancel<'a>(&'a self, _: &'a ExecutionHandleV1, _: u64) -> BackendFuture<'a, ()> {
        unavailable()
    }

    fn collect_artifacts<'a>(
        &'a self,
        _: &'a ExecutionHandleV1,
    ) -> BackendFuture<'a, ExecutionResultV1> {
        unavailable()
    }

    fn release<'a>(&'a self, _: &'a ExecutionHandleV1) -> BackendFuture<'a, ()> {
        unavailable()
    }
}

fn serve_with_journal(
    backend: Arc<dyn ExecutionBackend>,
    input: Vec<u8>,
    root: &std::path::Path,
) -> Vec<u8> {
    let mut output = Vec::new();
    serve_stdio_with_journal(
        backend,
        None,
        None,
        Cursor::new(input),
        &mut output,
        super::journal::RequestJournal::new(root.to_owned()),
    )
    .unwrap();
    output
}

fn ready<T: Send + 'static>(result: Result<T, ExecutionError>) -> BackendFuture<'static, T> {
    Box::pin(async move { result })
}

fn unavailable<T: Send + 'static>() -> BackendFuture<'static, T> {
    ready(Err(ExecutionError::new(
        crate::execution::errors::ExecutionErrorKind::EnvironmentUnavailable,
        "not available in this server test",
        false,
    )))
}

#[test]
fn request_failure_names_the_operation_and_preserves_error_policy() {
    let original = ExecutionError::new(
        crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
        format!("{}界", "x".repeat(4_100)),
        true,
    );
    let contextual = super::request_error(&EnvelopeKindV1::Exec, original);

    assert_eq!(
        contextual.kind,
        crate::execution::errors::ExecutionErrorKind::WorkerDisconnected
    );
    assert!(contextual.retryable);
    assert!(
        contextual
            .message
            .starts_with("Remote worker Exec request failed: ")
    );
    contextual.validate().unwrap();
}

fn envelope(execution_id: &str) -> EnvelopeV1 {
    EnvelopeV1 {
        version: protocol::PROTOCOL_VERSION_V1,
        request_id: "00000000-0000-4000-8000-000000000001".into(),
        execution_id: execution_id.into(),
        sequence: 1,
        kind: EnvelopeKindV1::Health,
        payload: serde_json::json!({"wire_version": 1}),
    }
}

#[test]
fn stdio_worker_answers_health_using_the_backend_probe() {
    let request = envelope("00000000-0000-4000-8000-000000000000");
    let mut input = Vec::new();
    protocol::write_frame(&mut input, &request).unwrap();
    let root = tempfile::tempdir().unwrap();
    let output = serve_with_journal(
        Arc::new(FakeBackend(Arc::new(AtomicUsize::new(0)))),
        input,
        root.path(),
    );
    let response = protocol::read_frame(&mut Cursor::new(output))
        .unwrap()
        .unwrap();
    assert_eq!(response.kind, EnvelopeKindV1::Acknowledge);
    assert_eq!(response.request_id, request.request_id);
    assert_eq!(response.payload["wire_version"], 1);
    assert_eq!(
        response.payload["authenticated_principal"],
        crate::execution::remote::worker::authenticated_ssh_principal().unwrap()
    );
    assert_eq!(response.payload["probe"]["available"], true);
    assert_eq!(response.payload["probe"]["architecture"], "amd64");
    assert_eq!(
        response.payload["probe"]["capabilities"]["docker_rootless"],
        true
    );
    assert_eq!(
        response.payload["probe"]["capabilities"]["policy_hash"],
        "sha256:policy"
    );
}

#[path = "tests/transfers.rs"]
mod transfers;

#[test]
fn server_reports_control_execution_identity_mismatch_without_dispatching() {
    let request = envelope("00000000-0000-4000-8000-000000000099");
    let mut input = Vec::new();
    protocol::write_frame(&mut input, &request).unwrap();
    let root = tempfile::tempdir().unwrap();
    let output = serve_with_journal(
        Arc::new(FakeBackend(Arc::new(AtomicUsize::new(0)))),
        input,
        root.path(),
    );
    let response = protocol::read_frame(&mut Cursor::new(output))
        .unwrap()
        .unwrap();
    assert_eq!(response.kind, EnvelopeKindV1::Error);
    assert_eq!(response.payload["kind"], "worker_incompatible");
}
