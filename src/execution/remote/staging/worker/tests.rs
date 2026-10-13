use super::*;

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
