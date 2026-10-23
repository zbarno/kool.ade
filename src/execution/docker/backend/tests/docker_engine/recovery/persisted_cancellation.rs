use super::*;
use crate::persistence::execution_records::ExecutionRecordStateV1;

/// Production boundary: persisted cancellation after a worker disappears with
/// a collected stage-out manifest but before terminal result persistence. Unit
/// tests cannot prove the fresh worker's Docker inspect, ownership validation,
/// workspace resynchronization, and durable cancellation result preserve that
/// manifest together. Requires native Linux Docker and the locked image cached
/// or pullable; moderate setup cost. No daemon restart, SSH, or credentials.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persisted_cancellation_keeps_previously_collected_artifact_manifest() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping Docker Engine integration; set {OPT_IN}=1 to opt in");
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 currently requires a native Linux Docker Engine");
    }

    let fixture = Fixture::new();
    let (workspace, _) = committed_workspace(&fixture);
    let (backend, task_clone) = backend_with_owner(
        &fixture,
        workspace.clone(),
        uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let owner_id = backend.identity.owner_instance_id.clone();
    let request = verification_request_with_artifacts(
        workspace.clone(),
        "from pathlib import Path; Path('result.txt').write_text('persisted-cancel-data\\n')",
        vec!["result.txt".into()],
    );
    let _cleanup = container_cleanup(&request);
    let (_lease, handle) = prepare_and_start(&backend, &request).await;
    let (stage_out_reached, _resume_finalization) =
        crate::execution::docker::backend::lifecycle::pause_after_stage_out(
            request.execution_id.clone(),
        );
    let backend = Arc::new(backend);
    let running = tokio::spawn({
        let backend = backend.clone();
        let handle = handle.clone();
        let request = request.clone();
        async move { backend.exec(&handle, &request).await }
    });
    tokio::time::timeout(Duration::from_secs(60), stage_out_reached)
        .await
        .expect("execution did not reach the persisted post-stage-out boundary")
        .expect("stage-out hook closed before reporting its boundary");

    let staged =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("stage-out execution record should be durable");
    assert_eq!(staged.state, ExecutionRecordStateV1::CollectingResults);
    assert_eq!(staged.artifacts.len(), 1);
    assert_eq!(staged.artifacts[0].relative_path, "result.txt");
    assert_eq!(staged.artifacts[0].size_bytes, 22);
    assert_eq!(
        staged.artifacts[0].media_type.as_deref(),
        Some("text/plain")
    );
    assert_eq!(
        fs::read_to_string(task_clone.join("evidence/result.txt")).unwrap(),
        "persisted-cancel-data\n"
    );

    // Model the worker disappearing after the manifest is committed. The
    // execution task is aborted and the old backend dropped, so cancel() on a
    // fresh backend takes cancel_persisted through lookup's disconnected path.
    running.abort();
    let _ = running.await;
    drop(backend);
    let (restarted, _) = backend_with_owner(&fixture, workspace.clone(), owner_id);
    restarted
        .cancel(&handle, request.cancellation_generation)
        .await
        .expect("fresh worker should cancel the persisted execution");

    let record =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("persisted cancellation record should remain durable");
    assert_eq!(record.state, ExecutionRecordStateV1::Cancelled);
    assert_eq!(record.artifacts, staged.artifacts);
    let result = restarted
        .collect_artifacts(&handle)
        .await
        .expect("persisted cancellation result should remain available");
    assert!(result.cancelled);
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].relative_path, "result.txt");
    assert_eq!(result.artifacts[0].size_bytes, 22);
    assert_eq!(result.artifacts[0].media_type, "text/plain");
    assert_eq!(
        result.artifacts[0].sha256,
        staged.artifacts[0].sha256.strip_prefix("sha256:").unwrap()
    );
    assert_eq!(
        record
            .result
            .as_ref()
            .map(|result| result.artifacts.as_slice()),
        Some(result.artifacts.as_slice())
    );
}
