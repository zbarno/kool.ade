use std::{os::unix::fs::PermissionsExt, path::Path};

use crate::execution::{
    contract::ResourceBudgetV1,
    docker::images::{ImageLock, SelectedImage},
};

#[path = "task_artifact/dispatch.rs"]
mod dispatch;
pub(crate) use dispatch::exercise_prepared_dispatch;

// Production boundary: request preparation consumes persisted reviewed task,
// target approval, Build/Start receipts, and artifact hashes before the
// app-owned rootless desktop driver uploads and exercises the selected bytes.
// Unit tests cannot prove Docker upload, target process ownership, Xvfb input,
// evidence capture, and container cleanup work together. External dependency:
// rootless Docker with the candidate image. Setup cost: one fixture binary
// build and one short desktop run after the image build.
pub(super) async fn exercise(
    docker: &bollard::Docker,
    image: &SelectedImage,
    lock: &ImageLock,
    resources: &ResourceBudgetV1,
) {
    let fixture_path = match std::env::var_os("KOOLADE_QA_DESKTOP_FIXTURE_EXECUTABLE") {
        Some(path) => path,
        None if std::env::var_os("KOOLADE_QA_REQUIRE_TASK_ARTIFACT").is_some() => {
            panic!("the rootless candidate job requires its separately built desktop fixture")
        }
        None => return,
    };
    let fixture_path = Path::new(&fixture_path);
    let metadata = std::fs::symlink_metadata(fixture_path).unwrap();
    assert!(metadata.is_file() && !metadata.file_type().is_symlink());
    assert!(metadata.permissions().mode() & 0o111 != 0);

    let fixture =
        crate::app::root::test_runtime_qa::DesktopArtifactFixture::from_executable(fixture_path);
    let dispatch: crate::app::root::test_runtime_qa::RootlessDesktopDispatch =
        crate::app::root::test_runtime_qa::prepare_rootless_desktop_dispatch(
            &fixture.project,
            &fixture.task,
        )
        .expect("production reviewed task request and approved artifact dispatch");
    assert_eq!(
        dispatch.request.relevant_platforms,
        [crate::runtime_qa::QaPlatform::LinuxDesktop]
    );
    assert_eq!(
        dispatch.request.target_id.as_deref(),
        Some("qa-desktop-fixture-target")
    );
    assert_eq!(
        dispatch.request.code_head_sha,
        fixture.task.reviewed_code_head_sha.as_deref().unwrap()
    );
    assert_eq!(
        dispatch.target.executable,
        "/workspace/dist/approved-desktop-app"
    );
    assert_eq!(dispatch.target.arguments, ["--fixture-window".to_owned()]);
    dispatch::run(docker, image, lock, resources, dispatch).await;
}
