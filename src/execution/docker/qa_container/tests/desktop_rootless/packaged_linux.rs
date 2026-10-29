use std::{os::unix::fs::PermissionsExt, path::Path};

use crate::execution::docker::images::RuntimeProfile;

// Production boundary: a language-neutral packaged Linux executable is bound
// to an approved task target and exact artifact receipt, then passed through
// production request/artifact preparation and the app-owned rootless desktop
// driver. Unit tests cannot prove the packaged ELF ABI, Xvfb window ownership,
// pointer/keyboard input, screenshot capture, and container cleanup together.
// External dependencies: rootless Docker and the locked desktop QA candidate.
// Setup cost: compile one small host-side C fixture and run one short Xvfb case.
#[tokio::test]
#[ignore = "requires rootless Docker and the loaded desktop QA candidate"]
async fn rootless_packaged_framework_neutral_linux_app_uses_production_dispatch() {
    let executable = std::env::var_os("KOOLADE_QA_PACKAGED_DESKTOP_EXECUTABLE")
        .expect("CI provides the packaged Linux desktop fixture");
    let executable = Path::new(&executable);
    let metadata = std::fs::symlink_metadata(executable).unwrap();
    assert!(metadata.is_file() && !metadata.file_type().is_symlink());
    assert_ne!(metadata.permissions().mode() & 0o111, 0);

    let docker = bollard::Docker::connect_with_local_defaults().unwrap();
    let info = docker.info().await.unwrap();
    assert!(
        info.security_options
            .unwrap_or_default()
            .iter()
            .any(|option| option.contains("name=rootless")),
        "packaged desktop QA must run on rootless Docker"
    );

    let fixture =
        crate::app::root::test_runtime_qa::DesktopArtifactFixture::from_packaged_executable(
            executable,
        );
    let runtime =
        crate::project_runtime::ProjectRuntimeStore::new(&fixture.project.state.planning_store);
    let manifest = runtime.load_manifest().unwrap().unwrap();
    assert!(manifest.detected_languages.is_empty());
    assert!(manifest.frameworks.is_empty());
    let target = manifest
        .candidate_targets
        .iter()
        .find(|target| target.target_id == "qa-desktop-fixture-target")
        .unwrap();
    assert_eq!(
        target.app_surface,
        crate::project_runtime::AppSurface::LinuxDesktop
    );
    assert!(target.fixture_ref.is_none());
    assert_eq!(
        manifest
            .user_overrides
            .iter()
            .find(|recipe| recipe.recipe_id == "qa-desktop-fixture-build")
            .unwrap()
            .executable_id,
        "packaged-binary"
    );

    let dispatch = crate::app::root::test_runtime_qa::prepare_rootless_desktop_dispatch(
        &fixture.project,
        &fixture.task,
    )
    .expect("production dispatch accepts the reviewed packaged-app receipts");
    assert_eq!(
        dispatch.request.relevant_platforms,
        [crate::runtime_qa::QaPlatform::LinuxDesktop]
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

    let lock = super::candidate_desktop_lock();
    let image = lock.select(RuntimeProfile::DesktopQa, "amd64").unwrap();
    let mut resources = super::resources();
    resources.process_count = 128;
    super::exercise_prepared_dispatch(&docker, &image, &lock, &resources, dispatch).await;
}
