use super::*;

/// Production boundary: cancellation arrives after stage-out synchronization
/// and durable manifest persistence, while success finalization is paused.
/// Unit tests cannot prove cancellation's Docker teardown and terminal record
/// retain the validated artifact result. Requires native Linux Docker and the
/// locked image cached or pullable; moderate setup cost. No SSH, credentials,
/// daemon restart, or OS process kill.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_after_stage_out_keeps_durable_artifact_manifest() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping Docker Engine integration; set {OPT_IN}=1 to opt in");
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 currently requires a native Linux Docker Engine");
    }

    let fixture = Fixture::new();
    let workspace = workspace();
    let (backend, task_clone) = backend_with_owner(
        &fixture,
        workspace.clone(),
        uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let request = verification_request_with_artifacts(
        workspace.clone(),
        "printf 'single-run\\n' >> command-count.txt; mkdir -p evidence; printf 'stage-out-data\\n' > evidence/result.txt",
        vec!["evidence/result.txt".into()],
    );
    let _cleanup = container_cleanup(&request);
    let (_lease, handle) = prepare_and_start(&backend, &request).await;
    let (stage_out_reached, resume_finalization) =
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
        .expect("stage-out test hook closed before reporting its boundary");

    backend
        .cancel(&handle, request.cancellation_generation)
        .await
        .expect("cancellation should own the paused execution");
    resume_finalization
        .send(())
        .expect("resume the paused success finalizer");
    tokio::time::timeout(Duration::from_secs(15), running)
        .await
        .expect("execution did not leave the post-stage-out boundary")
        .expect("execution task panicked")
        .expect("cancelled execution finalizer should complete");

    let result = backend
        .collect_artifacts(&handle)
        .await
        .expect("cancelled execution result should remain available");
    assert!(result.cancelled);
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].relative_path, "evidence/result.txt");
    assert_eq!(result.artifacts[0].size_bytes, 15);
    assert_eq!(result.artifacts[0].media_type, "text/plain");
    assert_eq!(
        fs::read_to_string(task_clone.join("evidence/result.txt")).unwrap(),
        "stage-out-data\n"
    );
    let record =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("cancelled record should retain artifact evidence");
    assert_eq!(record.artifacts.len(), 1);
    assert_eq!(
        record.artifacts[0].media_type.as_deref(),
        Some("text/plain")
    );
    assert_eq!(
        record
            .result
            .as_ref()
            .map(|result| result.artifacts.as_slice()),
        Some(result.artifacts.as_slice())
    );
}
