use super::*;
use crate::persistence::execution_records::{ExecutionCleanupStateV1, ExecutionRecordStateV1};

/// Production boundary: a replacement worker reads the actual terminal
/// execution journal/result and performs deferred Docker cleanup. Unit tests
/// cannot establish that the Docker command's artifact sync, durable result,
/// task-clone state, and owned-container release survive an app restart as one
/// lifecycle. Requires native Linux Docker and the locked image cached or
/// pullable; moderate setup cost. No daemon restart, SSH, or credentials.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_preserves_terminal_artifacts_and_completes_pending_cleanup() {
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
        "from pathlib import Path; Path('result.txt').write_text('durable-result\\n'); Path('command-count.txt').open('a').write('single-command\\n')",
        vec!["result.txt".into()],
    );
    let _cleanup = container_cleanup(&request);
    let (_lease, handle) = prepare_and_start(&backend, &request).await;
    let container_id = container_id(&backend, &handle);
    backend
        .exec(&handle, &request)
        .await
        .expect("command and artifact stage-out should complete");
    let result = backend
        .collect_artifacts(&handle)
        .await
        .expect("completed command result should be available");
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].relative_path, "result.txt");
    assert_eq!(
        fs::read_to_string(task_clone.join("evidence/result.txt")).unwrap(),
        "durable-result\n"
    );

    let before_restart =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("terminal result is durably linked to the task");
    assert_eq!(before_restart.state, ExecutionRecordStateV1::Succeeded);
    assert_eq!(before_restart.cleanup, ExecutionCleanupStateV1::Pending);
    assert_eq!(before_restart.result.as_ref(), Some(&result));
    assert!(
        before_restart
            .phase_timings
            .workspace_stage_out_ms
            .is_some()
    );
    drop(backend);

    let (restarted, _) = backend_with_owner(&fixture, workspace.clone(), owner_id);
    restarted
        .probe(&crate::execution::contract::ExecutionTarget::local())
        .await
        .expect("replacement worker should restore the terminal record");
    let recovered = restarted
        .collect_artifacts(&handle)
        .await
        .expect("terminal result should be available after restart");
    assert_eq!(recovered, result);
    assert_eq!(
        fs::read_to_string(task_clone.join("evidence/command-count.txt")).unwrap(),
        "single-command\n",
        "recovery must return the persisted result without rerunning the command"
    );
    assert_eq!(
        fs::read_to_string(task_clone.join("evidence/result.txt")).unwrap(),
        "durable-result\n"
    );
    restarted
        .release(&handle)
        .await
        .expect("replacement worker should remove the owned terminal container");
    assert!(
        restarted
            .docker
            .inspect_container(
                &container_id,
                Some(
                    bollard::query_parameters::InspectContainerOptionsBuilder::default()
                        .size(false)
                        .build(),
                ),
            )
            .await
            .is_err(),
        "terminal cleanup should remove the Docker container"
    );
    let after_cleanup =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("cleanup should preserve the durable result record");
    assert_eq!(after_cleanup.state, ExecutionRecordStateV1::Succeeded);
    assert_eq!(after_cleanup.cleanup, ExecutionCleanupStateV1::Complete);
    assert!(after_cleanup.phase_timings.cleanup_ms.is_some());
}
