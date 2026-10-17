use super::*;
use crate::persistence::execution_records::ExecutionRecordStateV1;

/// Production boundary: Docker output has been synchronized into the
/// authoritative task clone and its validated artifact manifest has been
/// persisted, but the terminal result has not. Unit tests cannot prove worker
/// recovery retains that evidence without replaying the command. Requires
/// native Linux Docker and the locked image cached or pullable; moderate setup
/// cost. The injected panic is after the complete synchronization and manifest
/// persistence; this does not simulate OS process termination or a partial
/// multi-file sync. No daemon restart, SSH, or credentials.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_after_stage_out_before_result_commit_preserves_clone_without_replay() {
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
    let owner_id = backend.identity.owner_instance_id.clone();
    let request = verification_request_with_artifacts(
        workspace.clone(),
        "from pathlib import Path; p = Path('evidence'); p.mkdir(exist_ok=True); (p / 'command-count.txt').open('a').write('single-run\\n'); (p / 'result.txt').write_text('stage-out-data\\n')",
        vec!["evidence/result.txt".into()],
    );
    let _cleanup = container_cleanup(&request);
    let (_lease, handle) = prepare_and_start(&backend, &request).await;
    crate::execution::docker::backend::lifecycle::inject_crash_after_stage_out(
        request.execution_id.clone(),
    );

    let backend = Arc::new(backend);
    let running_backend = backend.clone();
    let running_request = request.clone();
    let running_handle = handle.clone();
    let running = tokio::spawn(async move {
        running_backend
            .exec(&running_handle, &running_request)
            .await
    });
    let crash = running
        .await
        .expect_err("injected post-sync panic should terminate the execution task");
    assert!(
        crash.is_panic(),
        "the stage-out fault must bypass terminal handling"
    );

    assert_eq!(
        fs::read_to_string(task_clone.join("evidence/result.txt")).unwrap(),
        "stage-out-data\n",
        "stage-out must have synchronized the complete file before the crash point"
    );
    let before_restart =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("in-flight record remains durable at the injected crash point");
    assert_eq!(
        before_restart.state,
        ExecutionRecordStateV1::CollectingResults
    );
    assert!(before_restart.command_started);
    assert!(before_restart.result.is_none());
    assert_eq!(before_restart.artifacts.len(), 1);
    assert_eq!(
        before_restart.artifacts[0].relative_path,
        "evidence/result.txt"
    );
    assert_eq!(before_restart.artifacts[0].size_bytes, 15);
    assert_eq!(
        before_restart.artifacts[0].media_type.as_deref(),
        Some("text/plain")
    );
    drop(backend);

    let (restarted, _) = backend_with_owner(&fixture, workspace.clone(), owner_id);
    restarted
        .probe(&crate::execution::contract::ExecutionTarget::local())
        .await
        .expect("replacement worker should reconcile the interrupted stage-out");
    let recovered =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("interrupted stage-out record remains durable");
    assert_eq!(recovered.state, ExecutionRecordStateV1::Interrupted);
    assert!(recovered.command_started);
    assert_eq!(recovered.artifacts, before_restart.artifacts);
    assert_eq!(
        fs::read_to_string(task_clone.join("evidence/command-count.txt")).unwrap(),
        "single-run\n",
        "recovery must not replay the command after stage-out"
    );
    assert_eq!(
        fs::read_to_string(task_clone.join("evidence/result.txt")).unwrap(),
        "stage-out-data\n",
        "recovery must preserve the synchronized task-clone result"
    );
    let result = restarted
        .collect_artifacts(&handle)
        .await
        .expect("interrupted execution result should remain available");
    assert_eq!(
        result.failure.map(|failure| failure.kind),
        Some(crate::execution::errors::ExecutionErrorKind::WorkerDisconnected),
        "an incomplete result commit must remain interrupted, never successful"
    );
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].relative_path, "evidence/result.txt");
    assert_eq!(result.artifacts[0].size_bytes, 15);
    assert_eq!(result.artifacts[0].media_type, "text/plain");
    assert_eq!(
        result.artifacts[0].sha256,
        recovered.artifacts[0]
            .sha256
            .strip_prefix("sha256:")
            .unwrap()
    );
}
