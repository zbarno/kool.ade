use super::*;
use std::path::PathBuf;

struct Fixture(PathBuf, String);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "koolade-workspace-manifest-{}",
            uuid::Uuid::new_v4().hyphenated()
        ));
        fs::create_dir_all(&root).unwrap();
        let template = root.with_extension("template");
        let global_config = root.with_extension("gitconfig");
        fs::create_dir_all(&template).unwrap();
        fs::write(&global_config, b"").unwrap();
        let initialized = std::process::Command::new("git")
            .args(["init", "--quiet", "--initial-branch=main"])
            .arg(format!("--template={}", template.display()))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", &global_config)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_CONFIG_COUNT")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(initialized.success());
        let committed = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Workspace Fixture",
                "-c",
                "user.email=workspace-fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
            ])
            .arg(format!("core.hooksPath={}", template.display()))
            .args(["commit", "--quiet", "--allow-empty", "-m", "fixture"])
            .env_remove("GIT_AUTHOR_NAME")
            .env_remove("GIT_AUTHOR_EMAIL")
            .env_remove("GIT_COMMITTER_NAME")
            .env_remove("GIT_COMMITTER_EMAIL")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", root.with_extension("gitconfig"))
            .env_remove("GIT_CONFIG_COUNT")
            .env_remove("GIT_CONFIG_KEY_0")
            .env_remove("GIT_CONFIG_VALUE_0")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .current_dir(&root)
            .status()
            .unwrap();
        assert!(committed.success());
        let source_sha = std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", root.with_extension("gitconfig"))
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .env_remove("GIT_CONFIG_COUNT")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .current_dir(&root)
            .output()
            .unwrap();
        assert!(source_sha.status.success());
        let source_sha = String::from_utf8(source_sha.stdout)
            .unwrap()
            .trim()
            .to_owned();
        Self(root, source_sha)
    }

    fn workspace(&self) -> WorkspaceRef {
        WorkspaceRef {
            project_id: "00000000-0000-4000-8000-000000000001".into(),
            task_id: "00000000-0000-4000-8000-000000000002".into(),
            source_sha: self.1.clone(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
        let _ = fs::remove_dir_all(self.0.with_extension("template"));
        let _ = fs::remove_file(self.0.with_extension("gitconfig"));
    }
}

#[test]
fn captures_stable_file_metadata_and_records_private_exclusions() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("src")).unwrap();
    fs::write(fixture.0.join("src/lib.rs"), b"fn main() {}\n").unwrap();
    fs::write(fixture.0.join(".env.local"), b"not sent").unwrap();

    let manifest = WorkspaceManifestV1::capture(fixture.workspace(), &fixture.0).unwrap();

    assert_eq!(manifest.entries.len(), 2);
    assert_eq!(manifest.entries[1].path, "src/lib.rs");
    assert_eq!(
        manifest.entries[1].size_bytes,
        b"fn main() {}\n".len() as u64
    );
    assert_eq!(manifest.excluded_paths, [".env.local", ".git"]);
    manifest.validate().unwrap();
}

#[test]
fn workspace_transfer_manifest_binds_pinned_source_and_authenticated_peers() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("README.md"), b"public workspace content").unwrap();
    let workspace = WorkspaceManifestV1::capture(fixture.workspace(), &fixture.0).unwrap();
    let transfer = crate::execution::remote::staging::TransferManifestV1::workspace(
        "controller:desktop-1",
        "worker:build-1",
        workspace,
    )
    .unwrap();
    assert_eq!(transfer.workspace_ref().unwrap().source_sha, fixture.1);
    transfer.validate().unwrap();

    let mut rebound = transfer;
    rebound.source_identity = "worker:attacker".into();
    assert!(rebound.validate().is_err());

    let result = WorkspaceManifestV1::capture(fixture.workspace(), &fixture.0).unwrap();
    let result_transfer = crate::execution::remote::staging::TransferManifestV1::workspace_result(
        "worker:build-1",
        "controller:desktop-1",
        result,
    )
    .unwrap();
    assert!(result_transfer.workspace_ref().is_some());
}

#[test]
fn complete_workspace_result_is_returned_only_after_verified_materialization() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("result.txt"), b"verified output").unwrap();
    let result = WorkspaceManifestV1::capture(fixture.workspace(), &fixture.0).unwrap();
    let transfer = crate::execution::remote::staging::TransferManifestV1::workspace_result(
        "worker:build-1",
        "controller:desktop-1",
        result,
    )
    .unwrap();
    let controller_staging = tempfile::tempdir().unwrap();
    let mut receiver = crate::execution::remote::staging::StagingReceiver::begin(
        controller_staging.path(),
        transfer.clone(),
        "worker:build-1",
        "controller:desktop-1",
    )
    .unwrap();
    let mut sent = Vec::new();
    crate::execution::remote::staging::stream_file_chunks(
        &transfer,
        &fixture.0,
        "result.txt",
        |chunk| {
            sent.push(chunk.clone());
            Ok(())
        },
    )
    .unwrap();
    for chunk in sent {
        receiver.accept_chunk(&chunk).unwrap();
    }
    let applied_bytes = receiver
        .apply_workspace_result(
            &fixture.workspace(),
            || Ok(()),
            |verified| {
                assert_eq!(verified.workspace().source_sha, fixture.1);
                fs::read(verified.materialized_path().join("result.txt"))
                    .map_err(anyhow::Error::from)
            },
        )
        .unwrap();
    assert_eq!(applied_bytes, b"verified output");
}

#[test]
fn streams_only_manifest_files_and_rejects_content_changed_after_capture() {
    let fixture = Fixture::new();
    let path = fixture.0.join("src.rs");
    fs::write(&path, b"snapshot bytes").unwrap();
    let manifest = WorkspaceManifestV1::capture(fixture.workspace(), &fixture.0).unwrap();
    let mut streamed = Vec::new();

    manifest
        .stream_file(&fixture.0, "src.rs", |chunk| {
            streamed.extend_from_slice(chunk);
            Ok(())
        })
        .unwrap();
    assert_eq!(streamed, b"snapshot bytes");
    assert!(
        manifest
            .stream_file(&fixture.0, "missing.rs", |_| Ok(()))
            .is_err()
    );

    fs::write(&path, b"changed after capture").unwrap();
    assert!(
        manifest
            .stream_file(&fixture.0, "src.rs", |_| Ok(()))
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn rejects_workspace_case_collisions_and_escaping_symlinks() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("Readme"), b"one").unwrap();
    fs::write(fixture.0.join("README"), b"two").unwrap();
    assert!(WorkspaceManifestV1::capture(fixture.workspace(), &fixture.0).is_err());

    fs::remove_file(fixture.0.join("README")).unwrap();
    std::os::unix::fs::symlink("../../outside", fixture.0.join("escape")).unwrap();
    assert!(WorkspaceManifestV1::capture(fixture.workspace(), &fixture.0).is_err());
}
