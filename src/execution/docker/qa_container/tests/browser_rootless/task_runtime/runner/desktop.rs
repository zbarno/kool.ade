use crate::{
    execution::docker::images::{ImageLock, RuntimeProfile},
    project_runtime::{ProjectRuntimeExecutor, ProjectRuntimeStore, RuntimeOperation},
};

use super::super::setup::Fixture;

pub(in crate::execution::docker::qa_container::tests::browser_rootless::task_runtime) fn run_rust_desktop(
    fixture: Fixture,
) {
    let endpoint = super::super::rootless_action_endpoint();
    let _action_socket_environment = super::super::allow_rootless_action_socket(&endpoint);
    let docker = endpoint.connect().unwrap();
    let info = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(docker.info())
        .unwrap();
    assert!(
        info.security_options
            .unwrap_or_default()
            .iter()
            .any(|option| option.contains("name=rootless")),
        "Rust desktop acceptance must run on rootless Docker"
    );

    let rust_lock = ImageLock::bundled().unwrap();
    let build_session = super::runtime_session(
        &fixture,
        &rust_lock,
        RuntimeProfile::Rust,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let runtime_store = ProjectRuntimeStore::new(&fixture.planning);
    let executor = ProjectRuntimeExecutor::new(&runtime_store);
    let build_receipt = executor
        .execute(&build_session, &fixture.target_id, RuntimeOperation::Build)
        .unwrap()
        .remove(0);
    let test_receipt = executor
        .execute(&build_session, &fixture.target_id, RuntimeOperation::Test)
        .unwrap()
        .remove(0);

    let start_session = super::runtime_session(
        &fixture,
        &rust_lock,
        RuntimeProfile::Rust,
        &uuid::Uuid::new_v4().hyphenated().to_string(),
    );
    let start_receipt = executor
        .execute(&start_session, &fixture.target_id, RuntimeOperation::Start)
        .unwrap()
        .remove(0);
    for (receipt, operation, recipe_id) in [
        (
            &build_receipt,
            RuntimeOperation::Build,
            &fixture.build_recipe_id,
        ),
        (
            &test_receipt,
            RuntimeOperation::Test,
            &fixture.test_recipe_id,
        ),
        (
            &start_receipt,
            RuntimeOperation::Start,
            &fixture.start_recipe_id,
        ),
    ] {
        assert_eq!(receipt.operation, operation);
        assert_eq!(receipt.recipe_id.as_deref(), Some(recipe_id.as_str()));
        assert_eq!(
            receipt.state,
            crate::project_runtime::RuntimeReceiptState::Passed
        );
        assert_eq!(receipt.task_id, fixture.workspace.task_id);
        assert_eq!(receipt.target_id, fixture.target_id);
        assert_eq!(receipt.source_sha, fixture.workspace.source_sha);
        assert!(receipt.prepared_dependency_ids.is_empty());
    }
    let artifact_path = "target/release/runtime-qa-rust-desktop";
    assert_eq!(
        build_receipt
            .artifact_hashes
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [artifact_path]
    );
    assert_eq!(start_receipt.artifact_hashes, build_receipt.artifact_hashes);
    start_session.stop_runtime_processes().unwrap();

    let project = fixture
        .project
        .as_ref()
        .expect("desktop fixture has a reviewed lifecycle project");
    let task = fixture
        .lifecycle_task
        .as_ref()
        .expect("desktop fixture has a reviewed lifecycle task");
    let dispatch =
        crate::app::root::test_runtime_qa::prepare_rootless_desktop_dispatch(project, task)
            .expect("production dispatch accepts the executor Build/Start receipts");
    assert_eq!(
        dispatch.request.relevant_platforms,
        [crate::runtime_qa::QaPlatform::LinuxDesktop]
    );
    assert_eq!(dispatch.request.code_head_sha, fixture.workspace.source_sha);
    assert_eq!(
        dispatch.target.executable,
        format!("/workspace/{artifact_path}")
    );
    assert!(dispatch.target.arguments.is_empty());
    assert_eq!(dispatch.artifact_upload.target, "/workspace");

    let desktop_lock = super::super::super::super::desktop_rootless::candidate_desktop_lock();
    let desktop_image = desktop_lock
        .select(RuntimeProfile::DesktopQa, "amd64")
        .unwrap();
    let mut resources = crate::execution::docker::qa_container::tests::resources();
    resources.process_count = 128;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(
            super::super::super::super::desktop_rootless::exercise_prepared_dispatch(
                &docker,
                &desktop_image,
                &desktop_lock,
                &resources,
                dispatch,
            ),
        );
}
