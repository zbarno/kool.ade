use super::*;
use crate::execution::{
    contract::WorkspaceRef,
    remote::private_files::{
        PrivateDataClassV1, PrivateFileSpecV1, UserConfirmedPrivateFileGrantV1,
    },
};
use sha2::Digest;

fn confirmed(now: u64, workspace: WorkspaceRef) -> UserConfirmedPrivateFileGrantV1 {
    UserConfirmedPrivateFileGrantV1::after_confirmation(
        RemotePrivateFileGrantV1 {
            grant_id: "00000000-0000-4000-8000-000000000141".into(),
            controller_id: "00000000-0000-4000-8000-000000000142".into(),
            operator_id: String::new(),
            worker_id: "00000000-0000-4000-8000-000000000143".into(),
            worker_host_key_fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
                .into(),
            workspace,
            confirmed_at_unix_ms: 0,
            expires_at_unix_ms: now + 60_000,
            files: vec![PrivateFileSpecV1 {
                path: ".env.production".into(),
                size_bytes: 6,
                sha256: format!("{:x}", sha2::Sha256::digest(b"secret")),
                data_class: PrivateDataClassV1::CredentialFile,
            }],
        },
        "synthetic-operator-141".into(),
        now,
    )
    .unwrap()
}

fn git_source(root: &std::path::Path) -> WorkspaceManifestV1 {
    let gitconfig = root.with_extension("gitconfig");
    std::fs::write(&gitconfig, b"").unwrap();
    let initialized = std::process::Command::new("git")
        .args(["init", "--quiet", "--initial-branch=main"])
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .status()
        .unwrap();
    assert!(initialized.success());
    let committed = std::process::Command::new("git")
        .args([
            "-C",
            root.to_str().unwrap(),
            "-c",
            "user.name=Grant Fixture",
            "-c",
            "user.email=grant-fixture@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "fixture",
        ])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .output()
        .unwrap();
    assert!(committed.status.success());
    let head = std::process::Command::new("git")
        .args(["-C", root.to_str().unwrap(), "rev-parse", "HEAD"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", &gitconfig)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_CONFIG_COUNT")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .output()
        .unwrap();
    assert!(head.status.success());
    let source_sha = String::from_utf8(head.stdout).unwrap().trim().to_owned();
    let workspace = WorkspaceRef {
        project_id: "00000000-0000-4000-8000-000000000145".into(),
        task_id: "00000000-0000-4000-8000-000000000144".into(),
        source_sha,
    };
    WorkspaceManifestV1::capture(workspace, root).unwrap()
}

#[test]
#[cfg(unix)]
fn source_reader_requires_receipt_exclusion_and_exact_file_hash() {
    // Production boundary: proves controller source bytes are loaded only
    // through the durable grant, captured workspace exclusion set, and pinned
    // Git source. A validator test cannot cover handle-relative file reads or
    // repository identity. Uses a temporary Git repo; no network or SSH.
    let grants = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join(".env.production"), b"secret").unwrap();
    let snapshot = git_source(source.path());
    let now = crate::persistence::harness_settings::current_unix_ms();
    let confirmed = confirmed(now, snapshot.workspace.clone());
    let grant = crate::persistence::remote_private_file_grants::record_user_confirmation(
        grants.path(),
        confirmed,
        &snapshot.excluded_paths,
    )
    .unwrap();
    let bytes = read_authorized_private_file(
        grants.path(),
        source.path(),
        &grant,
        ".env.production",
        &snapshot,
        now + 1,
    )
    .unwrap();
    assert_eq!(bytes.as_slice(), b"secret");

    std::fs::remove_file(source.path().join(".env.production")).unwrap();
    let without_granted_path =
        WorkspaceManifestV1::capture(snapshot.workspace.clone(), source.path()).unwrap();
    assert!(
        read_authorized_private_file(
            grants.path(),
            source.path(),
            &grant,
            ".env.production",
            &without_granted_path,
            now + 1,
        )
        .is_err()
    );
    std::fs::write(source.path().join(".env.production"), b"secret").unwrap();
    std::fs::write(source.path().join(".env.production"), b"secreT").unwrap();
    assert!(
        read_authorized_private_file(
            grants.path(),
            source.path(),
            &grant,
            ".env.production",
            &snapshot,
            now + 1,
        )
        .is_err()
    );
}

#[test]
#[cfg(target_os = "linux")]
fn discovery_lists_exact_excluded_files_but_never_git_metadata() {
    // Production boundary: drives secure source discovery against a real task
    // clone, proving nested credential paths are listed with content digests,
    // while Git internals stay excluded from approval candidates. A path
    // validator alone cannot prove descriptor-relative enumeration. Uses a
    // temporary Git repository; no network or SSH.
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join(".env.production"), b"synthetic-value").unwrap();
    std::fs::create_dir(source.path().join(".aws")).unwrap();
    std::fs::write(
        source.path().join(".aws/credentials"),
        b"synthetic-credential",
    )
    .unwrap();
    let snapshot = git_source(source.path());
    let files = discover_private_files(&snapshot.workspace, source.path(), &snapshot).unwrap();
    assert_eq!(
        files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        [".aws/credentials", ".env.production"]
    );
    assert_eq!(files[0].data_class, PrivateDataClassV1::CredentialFile);
    assert_eq!(
        files[1].data_class,
        PrivateDataClassV1::ProjectConfiguration
    );
    assert_eq!(files[0].size_bytes, b"synthetic-credential".len() as u64);
    assert_eq!(
        files[0].sha256,
        format!("{:x}", sha2::Sha256::digest(b"synthetic-credential"))
    );
    assert!(!files.iter().any(|file| file.path.starts_with(".git/")));

    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("linked-secret"), b"do-not-follow").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("linked-secret"),
        source.path().join(".aws/external"),
    )
    .unwrap();
    let changed_snapshot =
        WorkspaceManifestV1::capture(snapshot.workspace.clone(), source.path()).unwrap();
    assert!(
        discover_private_files(
            &changed_snapshot.workspace,
            source.path(),
            &changed_snapshot,
        )
        .is_err()
    );
}
