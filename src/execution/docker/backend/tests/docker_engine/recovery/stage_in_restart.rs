use super::*;
use crate::persistence::execution_records::{ExecutionCleanupStateV1, ExecutionRecordStateV1};

/// Production boundary: interruption after Docker created the owned container
/// but before stage-in completion was durably recorded. This may occur during
/// dependency-cache setup or archive upload; the record does not distinguish
/// those subphases. A unit test cannot prove worker restart verifies Docker
/// ownership, discards an unconfirmed container, and retains the authoritative
/// host clone. Requires native Linux Docker and the locked image cached or
/// pullable; high setup cost because it snapshots and transfers a 48 MiB input
/// to widen the pre-completion window. No daemon restart, SSH, or credentials.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restart_before_stage_in_completion_discards_unconfirmed_container() {
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
    let host_file = task_clone.join("large-task-local-input.bin");
    let payload = (0..48 * 1024 * 1024)
        .map(|index| ((index * 31 + index / 251) % 251) as u8)
        .collect::<Vec<_>>();
    fs::write(&host_file, &payload).unwrap();
    drop(payload);

    let request = request(
        workspace.clone(),
        "printf 'must-not-start\\n' >> command-count.txt",
    );
    let _cleanup = container_cleanup(&request);
    let owner_id = backend.identity.owner_instance_id.clone();
    let lease = backend
        .prepare_workspace(&request.workspace, &request.target)
        .await
        .expect("prepare task workspace");
    let backend = Arc::new(backend);
    let starting_backend = backend.clone();
    let starting_lease = lease.clone();
    let starting_request = request.clone();
    let start = tokio::spawn(async move {
        starting_backend
            .start(&starting_lease, &starting_request)
            .await
    });

    let record = tokio::time::timeout(Duration::from_secs(60), async {
        loop {
            if let Some(record) = crate::persistence::execution_records::load(
                &workspace.task_id,
                &request.execution_id,
            )
            .expect("read in-progress execution record")
                && record.container_id.is_some()
                && record.state == ExecutionRecordStateV1::Preparing
                && record.phase_timings.workspace_stage_in_ms.is_none()
            {
                break record;
            }
            if start.is_finished() {
                panic!(
                    "Docker setup completed before its durable pre-stage-in boundary was observed"
                );
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("start did not reach the durable pre-stage-in-completion boundary");
    let container_id = record.container_id.expect("container identity was checked");
    assert!(!record.command_started);
    assert_eq!(record.cleanup, ExecutionCleanupStateV1::Pending);
    start.abort();
    let _ = start.await;
    drop(backend);

    let (restarted, _) = backend_with_owner(&fixture, workspace.clone(), owner_id);
    restarted
        .probe(&crate::execution::contract::ExecutionTarget::local())
        .await
        .expect("replacement worker should reconcile interrupted staging");
    let recovered =
        crate::persistence::execution_records::load(&workspace.task_id, &request.execution_id)
            .unwrap()
            .expect("interrupted staging record remains durable");
    assert_eq!(recovered.state, ExecutionRecordStateV1::Interrupted);
    assert!(!recovered.command_started);
    assert_eq!(recovered.cleanup, ExecutionCleanupStateV1::Complete);
    assert_eq!(
        fs::metadata(&host_file).unwrap().len(),
        48 * 1024 * 1024,
        "recovery must preserve the authoritative task clone"
    );
    assert!(!task_clone.join("command-count.txt").exists());
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
        "container without confirmed complete stage-in must be removed"
    );
}
