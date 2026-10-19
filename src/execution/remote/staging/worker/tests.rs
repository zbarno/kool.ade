use super::*;

#[test]
#[cfg(unix)]
fn unbound_private_stage_is_never_exposed_to_docker() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let stager = WorkerStager::new(root.path(), "fixture".into()).unwrap();
    let execution_id = "00000000-0000-4000-8000-000000000155";
    let private_root = stager.staging_root.join("private-files");
    create_private_dir(&private_root).unwrap();
    let execution_root = private_root.join(execution_id);
    fs::create_dir(&execution_root).unwrap();
    fs::write(execution_root.join("credentials"), b"synthetic-secret").unwrap();
    fs::set_permissions(&execution_root, fs::Permissions::from_mode(0o555)).unwrap();
    fs::set_permissions(
        execution_root.join("credentials"),
        fs::Permissions::from_mode(0o444),
    )
    .unwrap();

    assert!(
        stager
            .private_files_mount_root(execution_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn materialized_workspace_uses_fresh_git_metadata_not_controller_configuration() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::create_dir_all(&source).unwrap();
    let global = root.path().join("global-gitconfig");
    fs::write(&global, b"").unwrap();
    git_command(&source, &global)
        .args(["init", "--quiet", "--initial-branch=main"])
        .status()
        .unwrap();
    fs::write(source.join("README.md"), b"safe input\n").unwrap();
    git_command(&source, &global)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "add",
            "README.md",
        ])
        .status()
        .unwrap();
    git_command(&source, &global)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ])
        .status()
        .unwrap();
    let sha = String::from_utf8(
        git_command(&source, &global)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();

    let marker = root.path().join("hook-ran");
    fs::write(
        source.join(".git/config"),
        format!("[core]\n\thooksPath = {}\n", root.path().display()),
    )
    .unwrap();
    fs::create_dir_all(source.join(".git/hooks")).unwrap();
    let hook = source.join(".git/hooks/post-checkout");
    fs::write(&hook, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();

    let workspace = WorkspaceRef {
        project_id: "00000000-0000-4000-8000-000000000001".into(),
        task_id: "00000000-0000-4000-8000-000000000002".into(),
        source_sha: sha,
    };
    let snapshot =
        crate::execution::remote::WorkspaceManifestV1::capture(workspace.clone(), &source).unwrap();
    let transfer =
        TransferManifestV1::workspace("controller:fixture", "worker:fixture", snapshot).unwrap();
    let stager = WorkerStager::new(root.path(), "fixture".into()).unwrap();
    stager.begin(transfer.clone()).unwrap();
    for file in transfer.files() {
        super::super::sender::stream_file_chunks(&transfer, &source, file.path, |chunk| {
            stager.accept_chunk(chunk)
        })
        .unwrap();
    }
    stager.finalize(&transfer.transfer_id).unwrap();
    let staged = stager.resolve_owned_clone(&workspace).unwrap();

    let config = fs::read_to_string(staged.join(".git/config")).unwrap();
    assert!(!config.contains(root.path().to_str().unwrap()));
    assert!(!staged.join(".git/hooks/post-checkout").exists());
    assert!(!marker.exists());
    assert!(
        git_command(&staged, &global)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .status
            .success()
    );

    let generation = match &transfer.payload {
        TransferPayloadV1::Workspace(manifest) => manifest.snapshot_generation.clone(),
        _ => unreachable!(),
    };
    let result = stager
        .create_workspace_result(
            "00000000-0000-4000-8000-000000000003",
            &workspace,
            &generation,
            "controller:fixture",
        )
        .unwrap();
    let (path, _) = result.file_specs().into_iter().next().unwrap();
    assert!(
        stager
            .read_result_chunk(
                "00000000-0000-4000-8000-000000000004",
                &result.transfer_id,
                &workspace,
                path,
                0,
            )
            .is_err()
    );
    assert!(
        stager
            .read_result_chunk(
                "00000000-0000-4000-8000-000000000003",
                &result.transfer_id,
                &workspace,
                path,
                0,
            )
            .is_ok()
    );
    // Repeated preparation preserves retries and rejects replacement while
    // resolution, a lease, or an execution still owns the old generation.
    stager.finalize(&transfer.transfer_id).unwrap();
    fs::write(source.join("README.md"), b"fresh controller input\n").unwrap();
    let next_snapshot =
        crate::execution::remote::WorkspaceManifestV1::capture(workspace.clone(), &source).unwrap();
    let next = TransferManifestV1::workspace("controller:fixture", "worker:fixture", next_snapshot)
        .unwrap();
    let peer = WorkerStager::new(root.path(), "fixture".into()).unwrap();
    stager.begin(next.clone()).unwrap();
    peer.begin(next.clone()).unwrap();
    for file in next.files() {
        super::super::sender::stream_file_chunks(&next, &source, file.path, |chunk| {
            stager.accept_chunk(chunk)?;
            peer.accept_chunk(chunk)
        })
        .unwrap();
    }
    let reservation = stager.reserve_workspace(&workspace).unwrap();
    assert!(stager.finalize(&next.transfer_id).is_err());
    reservation.bind_lease("fixture-lease").unwrap();
    assert!(peer.finalize(&next.transfer_id).is_err());
    assert!(stager.finalize(&next.transfer_id).is_err());
    let execution_id = "00000000-0000-4000-8000-000000000191";
    stager
        .bind_execution("fixture-lease", execution_id)
        .unwrap();
    stager.release_workspace("fixture-lease").unwrap();
    assert!(peer.finalize(&next.transfer_id).is_err());
    assert!(stager.finalize(&next.transfer_id).is_err());
    assert_eq!(stager.resolve_owned_clone(&workspace).unwrap(), staged);
    stager.release_execution(execution_id).unwrap();
    stager.finalize(&next.transfer_id).unwrap();
    stager.finalize(&next.transfer_id).unwrap();
    let updated = stager.resolve_owned_clone(&workspace).unwrap();
    assert_ne!(updated, staged);
    assert_eq!(
        fs::read(updated.join("README.md")).unwrap(),
        b"fresh controller input\n"
    );
    assert_eq!(fs::read(staged.join("README.md")).unwrap(), b"safe input\n");
    let restarted = WorkerStager::new(root.path(), "fixture".into()).unwrap();
    assert_eq!(restarted.resolve_owned_clone(&workspace).unwrap(), updated);
    restarted.begin(transfer.clone()).unwrap();
    assert!(restarted.finalize(&transfer.transfer_id).is_err());
    // A previously created worker refreshes its cached generation under the
    // shared registry lock before issuing another lease.
    peer.begin(transfer.clone()).unwrap();
    assert!(peer.finalize(&transfer.transfer_id).is_err());
    let refreshed = peer.reserve_workspace(&workspace).unwrap();
    assert_eq!(peer.resolve_owned_clone(&workspace).unwrap(), updated);
    drop(refreshed);
}

fn git_command(root: &Path, global: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("git");
    command
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", global)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS");
    command
}
