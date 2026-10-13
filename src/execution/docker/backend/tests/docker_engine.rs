use std::{fs, sync::Arc, time::Duration};

use crate::execution::{
    backend::ExecutionBackend,
    contract::{EventPageRequestV1, ExecutionEventKindV1, ExecutionLifecycleV1},
};

#[path = "docker_engine/stack_recipes.rs"]
mod stack_recipes;
#[path = "docker_engine/support.rs"]
mod support;
use support::{
    Fixture, OPT_IN, backend, container_cleanup, container_id, prepare_and_start, request,
    wait_for_stdout, workspace,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn docker_engine_create_exec_cancel_and_cleanup_are_owned_and_bounded() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping Docker Engine integration; set {OPT_IN}=1 to opt in");
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 currently requires a native Linux Docker Engine");
    }

    let fixture = Fixture::new();
    let first_workspace = workspace();
    let (first_backend, first_clone) = backend(&fixture, first_workspace.clone());
    let host_secret = fixture.root.join("host-secret-canary");
    fs::write(&host_secret, "controller-only-canary").unwrap();
    let adversarial_checks = format!(
        "set -eu\n\
         test ! -S /var/run/docker.sock\n\
         test ! -S /run/user/1000/docker.sock\n\
         test ! -e /root/.ssh\n\
         test ! -e /home/executor/.aws\n\
         test ! -r '{}'\n\
         if touch .git/koolade-write-denied; then exit 41; fi\n\
         test ! -e .git/koolade-write-denied\n\
         route_lines=0\n\
         while IFS= read -r _; do route_lines=$((route_lines + 1)); done < /proc/net/route\n\
         test \"$route_lines\" -eq 1\n",
        host_secret.display()
    );
    let first_request = request(
        first_workspace,
        &format!(
            "{adversarial_checks}printf 'stdout-marker\\n'; printf 'stderr-marker\\n' >&2; printf 'synced\\n' > output.txt"
        ),
    );
    let _first_cleanup = container_cleanup(&first_request);
    let (_first_lease, first_handle) = prepare_and_start(&first_backend, &first_request).await;
    let first_container_id = container_id(&first_backend, &first_handle);
    let created = first_backend
        .docker
        .inspect_container(
            &first_container_id,
            Some(
                bollard::query_parameters::InspectContainerOptionsBuilder::default()
                    .size(false)
                    .build(),
            ),
        )
        .await
        .expect("inspect created container");
    let config = created.config.expect("container configuration");
    assert_eq!(config.user.as_deref(), Some("10001:10001"));
    let labels = config.labels.expect("container ownership labels");
    assert_eq!(
        labels.get(crate::execution::docker::sandbox::OWNER_LABEL),
        Some(&first_backend.identity.owner_instance_id)
    );
    assert_eq!(
        labels.get(crate::execution::docker::sandbox::EXECUTION_LABEL),
        Some(&first_request.execution_id)
    );
    assert_eq!(
        labels.get(crate::execution::docker::sandbox::TASK_LABEL),
        Some(&first_request.task_id)
    );
    let host = created.host_config.expect("container host configuration");
    assert_eq!(host.privileged, Some(false));
    assert_eq!(host.readonly_rootfs, Some(true));
    assert_eq!(host.network_mode.as_deref(), Some("none"));
    assert_eq!(
        host.pid_mode, None,
        "Docker defaults to a private PID namespace"
    );
    assert_eq!(host.ipc_mode.as_deref(), Some("private"));
    assert_eq!(
        host.cap_drop.as_deref(),
        Some(["ALL".to_owned()].as_slice())
    );
    assert!(host.cap_add.as_deref().unwrap_or_default().is_empty());
    assert!(
        host.security_opt
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|setting| setting == "no-new-privileges:true")
    );
    assert_eq!(host.memory, Some(512 * 1024 * 1024));
    assert_eq!(host.memory_swap, host.memory);
    assert_eq!(host.cpu_period, Some(100_000));
    assert_eq!(host.cpu_quota, Some(100_000));
    assert_eq!(host.pids_limit, Some(32));
    for mount in host.mounts.unwrap_or_default() {
        let source = mount.source.unwrap_or_default();
        assert!(!source.contains("docker.sock"));
        assert!(!source.starts_with("/home/") && !source.starts_with("/root/"));
    }
    first_backend
        .exec(&first_handle, &first_request)
        .await
        .expect("execute bounded shell script");
    let first_events = first_backend
        .events(&first_handle, EventPageRequestV1::new(0, 256).unwrap())
        .await
        .expect("read completed execution events");
    assert!(first_events.iter().any(|event| matches!(
        &event.event,
        ExecutionEventKindV1::Stdout { chunk } if chunk.contains("stdout-marker")
    )));
    assert!(first_events.iter().any(|event| matches!(
        &event.event,
        ExecutionEventKindV1::Stderr { chunk } if chunk.contains("stderr-marker")
    )));
    assert_eq!(
        fs::read_to_string(first_clone.join("output.txt")).unwrap(),
        "synced\n"
    );
    assert!(!first_clone.join(".git/koolade-write-denied").exists());
    assert_eq!(
        fs::read_to_string(host_secret).unwrap(),
        "controller-only-canary"
    );
    let result = first_backend
        .collect_artifacts(&first_handle)
        .await
        .expect("collect execution result");
    assert_eq!(result.exit_code, Some(0));
    assert!(!result.cancelled);
    first_backend
        .release(&first_handle)
        .await
        .expect("remove completed container");
    assert!(
        first_backend
            .docker
            .inspect_container(
                &first_container_id,
                Some(
                    bollard::query_parameters::InspectContainerOptionsBuilder::default()
                        .size(false)
                        .build(),
                ),
            )
            .await
            .is_err()
    );
    let second_workspace = workspace();
    let (second_backend, second_clone) = backend(&fixture, second_workspace.clone());
    let second_request = request(
        second_workspace,
        "printf 'cancel-ready\\n'; printf 'preserved\\n' > cancelled-output.txt; sleep 300",
    );
    let _second_cleanup = container_cleanup(&second_request);
    let (_second_lease, second_handle) = prepare_and_start(&second_backend, &second_request).await;
    let execution_backend = Arc::new(second_backend);
    let execution_request = second_request.clone();
    let execution_handle = second_handle.clone();
    let exec_backend = Arc::clone(&execution_backend);
    let running = tokio::spawn(async move {
        exec_backend
            .exec(&execution_handle, &execution_request)
            .await
    });
    wait_for_stdout(&execution_backend, &second_handle, "cancel-ready").await;
    execution_backend
        .cancel(&second_handle, second_request.cancellation_generation)
        .await
        .expect("cancel only the owned container");
    let _ = tokio::time::timeout(Duration::from_secs(15), running)
        .await
        .expect("exec stream did not end after cancellation");

    let result = execution_backend
        .collect_artifacts(&second_handle)
        .await
        .expect("collect cancelled execution result");
    assert!(result.cancelled);
    assert_eq!(
        fs::read_to_string(second_clone.join("cancelled-output.txt")).unwrap(),
        "preserved\n"
    );
    let terminal_events = execution_backend
        .events(&second_handle, EventPageRequestV1::new(0, 256).unwrap())
        .await
        .expect("read cancelled execution events")
        .into_iter()
        .filter(|event| {
            matches!(
                &event.event,
                ExecutionEventKindV1::Lifecycle {
                    state: ExecutionLifecycleV1::Succeeded
                        | ExecutionLifecycleV1::Failed
                        | ExecutionLifecycleV1::Cancelled
                        | ExecutionLifecycleV1::Interrupted
                }
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        terminal_events.len(),
        1,
        "terminal event must be exactly once"
    );
    assert!(matches!(
        &terminal_events[0].event,
        ExecutionEventKindV1::Lifecycle {
            state: ExecutionLifecycleV1::Cancelled
        }
    ));

    let container_id = container_id(&execution_backend, &second_handle);
    let stopped = execution_backend
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
        .expect("cancelled container remains inspectable until release");
    assert_eq!(stopped.state.and_then(|state| state.running), Some(false));
    execution_backend
        .release(&second_handle)
        .await
        .expect("release must remove the stopped container and its processes");
    assert!(
        execution_backend
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
            .is_err()
    );
}
