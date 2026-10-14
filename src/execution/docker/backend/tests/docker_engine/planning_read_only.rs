use std::{collections::BTreeSet, fs};

use super::support::{OPT_IN, backend, container_cleanup, prepare_and_start, request, workspace};
use crate::execution::{
    backend::ExecutionBackend,
    contract::{EventPageRequestV1, ExecutionEventKindV1, ExecutionModeV1},
    policy::ExecutionPolicyV1,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn planning_workspace_is_readable_but_not_writable_after_staging() {
    if std::env::var(OPT_IN).ok().as_deref() != Some("1") {
        eprintln!("skipping Docker Engine integration; set {OPT_IN}=1 to opt in");
        return;
    }
    if !cfg!(target_os = "linux") {
        panic!("{OPT_IN}=1 currently requires a native Linux Docker Engine");
    }

    let fixture = super::support::Fixture::new();
    let workspace = workspace();
    let (backend, task_clone) = backend(&fixture, workspace.clone());
    fs::write(task_clone.join("planning-input.txt"), "readable\n").unwrap();
    let script = "set -eu\n\
        test \"$(cat planning-input.txt)\" = readable\n\
        if touch planning-write-denied.txt; then exit 41; fi\n\
        test ! -e planning-write-denied.txt\n\
        printf 'planning-read-only-ok\\n'\n";
    let mut request = request(workspace, script);
    request.mode = ExecutionModeV1::PlanningReadOnly;
    request.policy = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::PlanningReadOnly,
        request.resource_budget.clone(),
        BTreeSet::new(),
    )
    .unwrap();
    let _cleanup = container_cleanup(&request);
    let (_lease, handle) = prepare_and_start(&backend, &request).await;
    backend.exec(&handle, &request).await.unwrap();
    let events = backend
        .events(&handle, EventPageRequestV1::new(0, 256).unwrap())
        .await
        .unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.event,
        ExecutionEventKindV1::Stdout { chunk } if chunk.contains("planning-read-only-ok")
    )));
    assert!(!task_clone.join("planning-write-denied.txt").exists());
    backend.release(&handle).await.unwrap();
}
