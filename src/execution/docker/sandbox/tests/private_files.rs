use super::*;
use crate::execution::docker::sandbox::{PRIVATE_FILES_TARGET, create_body_with_private_files};

#[test]
fn authorized_private_file_stage_is_a_separate_read_only_container_mount() {
    let fixture = Fixture::new();
    let workspace = ValidatedWorkspace::inspect(&fixture.0).unwrap();
    let private_files = tempfile::tempdir().unwrap();
    fs::write(
        private_files.path().join("credentials"),
        b"synthetic-secret",
    )
    .unwrap();
    let body = create_body_with_private_files(
        &request(ExecutionModeV1::Implementation),
        &workspace,
        &identity(),
        &selected_image(),
        None,
        Some(private_files.path()),
    )
    .unwrap();
    let mounts = body.host_config.unwrap().mounts.unwrap();
    let private_mount = mounts
        .iter()
        .find(|mount| mount.target.as_deref() == Some(PRIVATE_FILES_TARGET))
        .unwrap();

    assert_eq!(
        private_mount.source.as_deref(),
        Some(private_files.path().to_str().unwrap())
    );
    assert_eq!(private_mount.read_only, Some(true));
    assert_eq!(
        private_mount
            .bind_options
            .as_ref()
            .unwrap()
            .create_mountpoint,
        Some(true)
    );
    assert_eq!(mounts.len(), 2);
}
