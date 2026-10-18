use super::support::{
    Fixture, OPT_IN, container_cleanup, container_id, request, wait_for_stdout_with_task, workspace,
};
use crate::execution::{
    backend::ExecutionBackend,
    docker::{
        backend::{DockerExecutionBackend, OwnedTaskWorkspaceResolver},
        engine::{DockerEndpoint, HostPlatform},
    },
};
use futures_util::StreamExt;
use std::{fs, path::Path, process::Command, sync::Arc, time::Duration};

/// Production boundary: app-owned task clone resolution, two Docker container
/// mounts, bounded Docker stats collection, cancellation of one execution
/// stream, and result synchronization. A unit test cannot prove that the live
/// daemon keeps same-named branches and writable files isolated across mounts
/// or supplies actual container metrics. Requires native Linux Docker, the Git
/// CLI, and the locked image to be cached or pullable; moderate setup cost. No
/// SSH, network access from the containers, or credentials are required.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_one_parallel_task_preserves_the_other_same_named_branch_clone() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping Docker Engine integration; set {OPT_IN}=1 to opt in");
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 currently requires a native Linux Docker Engine");
    }

    let fixture = Fixture::new();
    let task_a = workspace();
    let task_b = workspace();
    assert_ne!(task_a.task_id, task_b.task_id);
    let clone_a = fixture.task_clone(&task_a);
    let clone_b = fixture.task_clone(&task_b);
    init_branch(&clone_a);
    init_branch(&clone_b);

    let projects = fixture.root.join("koolade-home/projects");
    let resolver_a = Arc::new(
        OwnedTaskWorkspaceResolver::new(&projects, task_a.clone(), &clone_a)
            .expect("registered task A clone"),
    );
    let resolver_b = Arc::new(
        OwnedTaskWorkspaceResolver::new(&projects, task_b.clone(), &clone_b)
            .expect("registered task B clone"),
    );
    let owner_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let backend_a = Arc::new(
        DockerExecutionBackend::connect(
            DockerEndpoint::from_process_environment(HostPlatform::Linux)
                .expect("opted-in test requires a supported local Docker Engine"),
            resolver_a,
            owner_id.clone(),
        )
        .expect("connect task A backend"),
    );
    let backend_b = Arc::new(
        DockerExecutionBackend::connect(
            DockerEndpoint::from_process_environment(HostPlatform::Linux)
                .expect("opted-in test requires a supported local Docker Engine"),
            resolver_b,
            owner_id,
        )
        .expect("connect task B backend"),
    );

    let request_a = request(
        task_a.clone(),
        "printf 'task-a-ready\\n'; printf 'only-a\\n' > task-a.txt; sleep 300",
    );
    let request_b = request(
        task_b.clone(),
        "printf 'only-b\\n' > task-b.txt; printf 'task-b-complete\\n'",
    );
    let _cleanup_a = container_cleanup(&request_a);
    let _cleanup_b = container_cleanup(&request_b);
    let (lease_a, lease_b) = tokio::join!(
        backend_a.prepare_workspace(&task_a, &request_a.target),
        backend_b.prepare_workspace(&task_b, &request_b.target),
    );
    let (lease_a, lease_b) = (
        lease_a.expect("prepare task A"),
        lease_b.expect("prepare task B"),
    );
    let (handle_a, handle_b) = tokio::join!(
        backend_a.start(&lease_a, &request_a),
        backend_b.start(&lease_b, &request_b),
    );
    let (handle_a, handle_b) = (
        handle_a.expect("start task A"),
        handle_b.expect("start task B"),
    );

    let exec_a_backend = Arc::clone(&backend_a);
    let exec_a_handle = handle_a.clone();
    let exec_a_request = request_a.clone();
    let running_a =
        tokio::spawn(async move { exec_a_backend.exec(&exec_a_handle, &exec_a_request).await });
    let exec_b_backend = Arc::clone(&backend_b);
    let exec_b_handle = handle_b.clone();
    let exec_b_request = request_b.clone();
    let running_b =
        tokio::spawn(async move { exec_b_backend.exec(&exec_b_handle, &exec_b_request).await });
    wait_for_stdout_with_task(&backend_a, &handle_a, "task-a-ready", &running_a).await;
    wait_for_stdout_with_task(&backend_b, &handle_b, "task-b-complete", &running_b).await;
    wait_for_sample(
        backend_a.as_ref(),
        &handle_a,
        &task_a.task_id,
        &request_a.execution_id,
        &running_a,
    )
    .await;

    backend_a
        .cancel(&handle_a, request_a.cancellation_generation)
        .await
        .expect("cancel only task A");
    tokio::time::timeout(Duration::from_secs(15), running_a)
        .await
        .expect("task A stream did not close after cancellation")
        .expect("task A execution panicked")
        .expect("task A cancellation should close its stream cleanly");
    tokio::time::timeout(Duration::from_secs(15), running_b)
        .await
        .expect("task B failed to finish independently")
        .expect("task B execution panicked")
        .expect("task B execution should succeed");

    let result_a = backend_a
        .collect_artifacts(&handle_a)
        .await
        .expect("collect task A");
    let result_b = backend_b
        .collect_artifacts(&handle_b)
        .await
        .expect("collect task B");
    assert!(result_a.cancelled);
    assert_eq!(result_b.exit_code, Some(0));
    assert_eq!(
        fs::read_to_string(clone_a.join("task-a.txt")).unwrap(),
        "only-a\n"
    );
    assert_eq!(
        fs::read_to_string(clone_b.join("task-b.txt")).unwrap(),
        "only-b\n"
    );
    assert!(!clone_a.join("task-b.txt").exists());
    assert!(!clone_b.join("task-a.txt").exists());

    let record_a =
        crate::persistence::execution_records::load(&task_a.task_id, &request_a.execution_id)
            .expect("read task A record")
            .expect("task A record is durable");
    let record_b =
        crate::persistence::execution_records::load(&task_b.task_id, &request_b.execution_id)
            .expect("read task B record")
            .expect("task B record is durable");
    assert_eq!(record_a.task_id, task_a.task_id);
    assert_eq!(record_b.task_id, task_b.task_id);
    assert_eq!(
        record_a.state,
        crate::persistence::execution_records::ExecutionRecordStateV1::Cancelled
    );
    assert_eq!(
        record_b.state,
        crate::persistence::execution_records::ExecutionRecordStateV1::Succeeded
    );
    assert_eq!(record_a.exit_code, None);
    assert_eq!(record_b.exit_code, Some(0));
    assert!(record_a.docker_usage.sample_count > 0);
    assert!(
        record_a.docker_usage.cpu_time_ms.is_some()
            || record_a.docker_usage.sampled_peak_memory_bytes.is_some()
            || record_a.docker_usage.latest_process_count.is_some()
    );
    assert!(record_a.phase_timings.image_preparation_ms.is_some());
    assert!(record_a.phase_timings.dependency_cache_stage_ms.is_some());
    assert!(record_a.phase_timings.workspace_stage_in_ms.is_some());
    assert!(record_a.phase_timings.workspace_stage_out_ms.is_some());

    backend_a
        .release(&handle_a)
        .await
        .expect("release task A container");
    backend_b
        .release(&handle_b)
        .await
        .expect("release task B container");
    let released_a =
        crate::persistence::execution_records::load(&task_a.task_id, &request_a.execution_id)
            .expect("read task A cleanup record")
            .expect("task A cleanup record remains durable");
    assert!(released_a.phase_timings.cleanup_ms.is_some());
}

async fn wait_for_sample(
    backend: &DockerExecutionBackend,
    handle: &crate::execution::backend::ExecutionHandleV1,
    task_id: &str,
    execution_id: &str,
    exec_task: &tokio::task::JoinHandle<Result<(), crate::execution::errors::ExecutionError>>,
) {
    let collected = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let record = crate::persistence::execution_records::load(task_id, execution_id)
                .expect("read durable Docker telemetry");
            if record.is_some_and(|record| record.docker_usage.sample_count > 0) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    if collected.is_ok() {
        return;
    }

    let durable_record = crate::persistence::execution_records::load(task_id, execution_id)
        .expect("read durable Docker telemetry after timeout")
        .map(|record| {
            (
                record.docker_usage.sample_count,
                record.state,
                record.command_started,
            )
        });
    let metric_progress = backend.diagnostic_metrics(handle).unwrap_or_default();
    let execution_progress = backend.diagnostic_progress(handle).unwrap_or_default();

    let mut stats = backend.docker.stats(
        &container_id(backend, handle),
        Some(
            bollard::query_parameters::StatsOptionsBuilder::default()
                .stream(true)
                .build(),
        ),
    );
    let probe = match tokio::time::timeout(Duration::from_secs(3), stats.next()).await {
        Err(_) => "direct stats probe timed out".to_owned(),
        Ok(None) => "direct stats stream ended".to_owned(),
        Ok(Some(Err(_))) => "direct stats probe returned an error".to_owned(),
        Ok(Some(Ok(sample))) => {
            let has_cpu = sample
                .cpu_stats
                .as_ref()
                .and_then(|stats| stats.cpu_usage.as_ref())
                .and_then(|usage| usage.total_usage)
                .is_some();
            let has_memory = sample
                .memory_stats
                .as_ref()
                .and_then(|stats| stats.max_usage.or(stats.usage))
                .is_some();
            let has_processes = sample
                .pids_stats
                .as_ref()
                .and_then(|stats| stats.current)
                .is_some();
            format!(
                "direct stats response (os_type={:?}, cpu={has_cpu}, memory={has_memory}, processes={has_processes})",
                sample.os_type
            )
        }
    };
    panic!(
        "Docker did not emit a bounded resource metric sample; {probe}; durable_record={durable_record:?}; metrics_progress_mask={metric_progress:#06x}; execution_progress_mask={execution_progress:#04x}; exec_task_finished={}",
        exec_task.is_finished(),
    );
}

fn init_branch(path: &Path) {
    let output = Command::new("git")
        .arg("init")
        .arg("--initial-branch=feature/shared-name")
        .arg(path)
        .output()
        .expect("invoke Git for isolated test clone");
    assert!(
        output.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(
            &Command::new("git")
                .args(["-C", path.to_str().unwrap(), "branch", "--show-current"])
                .output()
                .expect("read test branch")
                .stdout,
        )
        .trim(),
        "feature/shared-name"
    );
}
