use std::sync::{Arc, Mutex};

use crate::{
    execution::{
        backend::{
            BackendFuture, BackendProbeV1, ExecutionBackend, ExecutionHandleV1, WorkspaceLeaseV1,
        },
        contract::{
            EventPageRequestV1, ExecutionEventV1, ExecutionModeV1, ExecutionRequestV1,
            ExecutionResultV1, ExecutionTarget, ResourceBudgetV1, WorkspaceRef,
        },
        docker::{
            images::{RuntimeProfile, SelectedImage},
            qa_container::{
                DriverContainerOutput, QaArtifactMount, QaContainerUpload, QaDriverContainerSpec,
            },
        },
        session::ExecutionSession,
    },
    runtime_qa::{QaScenarioDriver, QaTargetAuthorization, RuntimeQaStore},
};

#[path = "tests/fixtures.rs"]
mod fixtures;
use fixtures::{driver_output, failure, request, scenario, target, uuid};

#[derive(Default)]
struct Observations {
    phases: Vec<&'static str>,
    upload_targets: Vec<String>,
    uploaded_request: Option<Vec<u8>>,
    uploaded_artifact: Option<Vec<u8>>,
    selected_image: Option<String>,
}

struct FakeBackend(Arc<Mutex<Observations>>);

impl ExecutionBackend for FakeBackend {
    fn create_qa_driver<'a>(
        &'a self,
        spec: &'a QaDriverContainerSpec<'a>,
        uploads: &'a [QaContainerUpload],
        _: &'a mut tokio::sync::watch::Receiver<bool>,
    ) -> BackendFuture<'a, String> {
        let observation = Arc::clone(&self.0);
        Box::pin(async move {
            let mut observation = observation.lock().unwrap();
            observation.phases.push("upload");
            observation.selected_image = Some(spec.image.digest.clone());
            observation.upload_targets =
                uploads.iter().map(|upload| upload.target.clone()).collect();
            let artifact_upload = uploads
                .iter()
                .find(|upload| upload.target == "/workspace")
                .ok_or_else(|| failure("task artifact upload is missing"))?;
            let mut archive = tar::Archive::new(artifact_upload.archive.as_slice());
            let mut artifact = archive
                .entries()
                .map_err(|_| failure("task artifact upload is invalid"))?
                .next()
                .ok_or_else(|| failure("task artifact is missing"))?
                .map_err(|_| failure("task artifact is invalid"))?;
            let mut artifact_bytes = Vec::new();
            std::io::Read::read_to_end(&mut artifact, &mut artifact_bytes)
                .map_err(|_| failure("task artifact could not be read"))?;
            observation.uploaded_artifact = Some(artifact_bytes);
            let request_upload = uploads
                .iter()
                .find(|upload| upload.target == "/run/koolade-runtime-qa")
                .ok_or_else(|| failure("QA request upload is missing"))?;
            let mut archive = tar::Archive::new(request_upload.archive.as_slice());
            let mut request = archive
                .entries()
                .map_err(|_| failure("QA request upload is invalid"))?
                .next()
                .ok_or_else(|| failure("QA request file is missing"))?
                .map_err(|_| failure("QA request file is invalid"))?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut request, &mut bytes)
                .map_err(|_| failure("QA request file could not be read"))?;
            observation.uploaded_request = Some(bytes);
            Ok("owned-container".into())
        })
    }

    fn run_qa_driver<'a>(
        &'a self,
        container_id: &'a str,
        _: &'a QaDriverContainerSpec<'a>,
        _: &'a mut tokio::sync::watch::Receiver<bool>,
    ) -> BackendFuture<'a, DriverContainerOutput> {
        let observation = Arc::clone(&self.0);
        Box::pin(async move {
            if container_id != "owned-container" {
                return Err(failure("driver attempted to read an unowned container"));
            }
            observation.lock().unwrap().phases.push("read");
            Ok(DriverContainerOutput {
                exit_code: 0,
                stdout: serde_json::to_vec(&driver_output())
                    .map_err(|_| failure("driver response could not be encoded"))?,
                stderr: Vec::new(),
            })
        })
    }

    fn cleanup_qa_driver<'a>(&'a self, _: &'a str) -> BackendFuture<'a, ()> {
        let observation = Arc::clone(&self.0);
        Box::pin(async move {
            observation.lock().unwrap().phases.push("cleanup");
            Ok(())
        })
    }

    fn probe(&self, _: &ExecutionTarget) -> BackendFuture<'_, BackendProbeV1> {
        Box::pin(async {
            Ok(BackendProbeV1 {
                wire_version: 1,
                available: true,
                compatible: true,
                architecture: Some("amd64".into()),
                failure: None,
                capabilities: None,
            })
        })
    }

    fn prepare_workspace<'a>(
        &'a self,
        workspace: &'a WorkspaceRef,
        _: &'a ExecutionTarget,
    ) -> BackendFuture<'a, WorkspaceLeaseV1> {
        Box::pin(async move {
            Ok(WorkspaceLeaseV1 {
                lease_id: "lease".into(),
                workspace: workspace.clone(),
                expires_unix_ms: u64::MAX,
            })
        })
    }

    fn start<'a>(
        &'a self,
        _: &'a WorkspaceLeaseV1,
        _: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ExecutionHandleV1> {
        Box::pin(async { Err(failure("unexpected ordinary execution")) })
    }

    fn release_workspace<'a>(&'a self, _: &'a WorkspaceLeaseV1) -> BackendFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn exec<'a>(
        &'a self,
        _: &'a ExecutionHandleV1,
        _: &'a ExecutionRequestV1,
    ) -> BackendFuture<'a, ()> {
        Box::pin(async { Err(failure("unexpected ordinary execution")) })
    }

    fn events<'a>(
        &'a self,
        _: &'a ExecutionHandleV1,
        _: EventPageRequestV1,
    ) -> BackendFuture<'a, Vec<ExecutionEventV1>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn cancel<'a>(&'a self, _: &'a ExecutionHandleV1, _: u64) -> BackendFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }

    fn collect_artifacts<'a>(
        &'a self,
        _: &'a ExecutionHandleV1,
    ) -> BackendFuture<'a, ExecutionResultV1> {
        Box::pin(async { Err(failure("unexpected ordinary execution")) })
    }

    fn release<'a>(&'a self, _: &'a ExecutionHandleV1) -> BackendFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

/// Production boundary: the app-owned QA adapter to the selected execution backend.
/// Invariant: only the exact request and artifact are uploaded, output is decoded from the owned
/// driver, and cleanup follows execution. Unit coverage cannot prove Docker's rootless isolation;
/// that still requires the hosted rootless image job. No external service is used; setup is a small
/// in-process backend fixture.
#[test]
fn docker_driver_uploads_request_reads_structured_output_and_cleans_owned_container() {
    let request = request();
    let task_id = request.parent_task_uid.clone();
    let image_digest = format!("sha256:{}", "a".repeat(64));
    let image = SelectedImage {
        profile: RuntimeProfile::DesktopQa,
        platform: "linux/amd64".into(),
        reference: format!("ghcr.io/zbarno/koolade-runtime-qa-desktop@{image_digest}"),
        digest: image_digest,
        runtime_user: "10001:10001".into(),
        tool_versions: Default::default(),
    };
    let observations = Arc::new(Mutex::new(Observations::default()));
    let workspace = WorkspaceRef {
        project_id: uuid(),
        task_id: task_id.clone(),
        source_sha: request.code_head_sha.clone(),
    };
    let session = ExecutionSession::new(
        Arc::new(FakeBackend(Arc::clone(&observations))),
        workspace,
        ExecutionTarget::local(),
        ExecutionModeV1::RuntimeQa,
        image.digest.clone(),
        ResourceBudgetV1::default(),
    )
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("app"), b"fixture executable").unwrap();
    let artifact_mount = QaArtifactMount::inspect(temp.path(), "/workspace").unwrap();
    let (artifact_upload, _) = artifact_mount.snapshot_upload(1024, None).unwrap();
    let target = target(&request);
    let scenario = scenario();
    let store = RuntimeQaStore::new(temp.path().join("qa-evidence"));
    let mut cancellation = tokio::sync::watch::channel(false).1;
    let mut driver = super::DockerQaScenarioDriver {
        session: &session,
        request: &request,
        image: &image,
        artifact_mount,
        artifact_upload: &artifact_upload,
        resources: &ResourceBudgetV1::default(),
        desktop_target: Some(&target),
        service_lease: None,
        relay_binding: None,
        store: &store,
        cancellation: &mut cancellation,
    };
    let output = driver
        .run_scenario(
            &request,
            &scenario,
            &QaTargetAuthorization::LinuxDesktop(&target),
        )
        .unwrap();
    assert_eq!(output.driver_version, request.driver_profile_version);
    assert!(output.completed);
    let observation = observations.lock().unwrap();
    assert_eq!(observation.phases, ["upload", "read", "cleanup"]);
    assert_eq!(
        observation.upload_targets,
        ["/workspace", "/run/koolade-runtime-qa"]
    );
    assert_eq!(
        observation.selected_image.as_deref(),
        Some(image.digest.as_str())
    );
    assert_eq!(
        observation.uploaded_artifact.as_deref(),
        Some(b"fixture executable".as_slice())
    );
    let uploaded: serde_json::Value =
        serde_json::from_slice(observation.uploaded_request.as_ref().unwrap()).unwrap();
    assert_eq!(uploaded["request"], serde_json::to_value(&request).unwrap());
    assert_eq!(uploaded["scenario"]["id"], scenario.id);
    assert_eq!(uploaded["target"]["executable"], target.executable);
}
