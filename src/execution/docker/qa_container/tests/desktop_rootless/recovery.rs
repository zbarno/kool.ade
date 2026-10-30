use std::time::Duration;

use crate::execution::docker::qa_container::{
    cleanup_driver_container, create_driver_container, recover_driver_container,
};

use super::super::super::QaDriverContainerSpec;
use crate::execution::docker::images::ImageLock;

// Production boundary: after a restart, recovery may remove only the orphan
// driver container whose deterministic name and task-scoped ownership labels
// match the current QA request. Unit tests cannot prove Docker inspection and
// forced cleanup affect the live container. External dependency: rootless
// Docker with the pinned desktop QA candidate loaded. Setup cost: two idle
// containers using the already-built candidate, removed during this scenario.
pub(super) async fn exercise(
    docker: &bollard::Docker,
    spec: &QaDriverContainerSpec<'_>,
    lock: &ImageLock,
) {
    let name = format!("koolade-qa-{}", spec.request.qa_run_id);
    let (_cancel, mut cancel) = tokio::sync::watch::channel(false);

    let mut mislabeled = spec.create_body_with_lock(lock).unwrap();
    mislabeled
        .labels
        .get_or_insert_default()
        .insert("com.koolade.qa.owner".into(), "foreign-owner".into());
    let foreign_id = create_driver_container(
        docker,
        &name,
        mislabeled,
        &[],
        Duration::from_secs(30),
        1024,
        &mut cancel,
    )
    .await
    .unwrap();

    recover_driver_container(docker, &name, spec.request, spec.owner_id, spec.driver)
        .await
        .unwrap();
    assert!(docker.inspect_container(&name, None).await.is_ok());
    cleanup_driver_container(docker, &foreign_id).await.unwrap();

    create_driver_container(
        docker,
        &name,
        spec.create_body_with_lock(lock).unwrap(),
        &[],
        Duration::from_secs(30),
        1024,
        &mut cancel,
    )
    .await
    .unwrap();
    recover_driver_container(docker, &name, spec.request, spec.owner_id, spec.driver)
        .await
        .unwrap();
    match docker.inspect_container(&name, None).await {
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => {}
        other => panic!("owned orphan QA container remains after recovery: {other:?}"),
    }
}
