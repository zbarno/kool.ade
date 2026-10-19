use super::*;
use sha2::{Digest, Sha512};
use std::path::{Path, PathBuf};

pub(super) fn prepare_workspace(workspace: &Path) -> (PathBuf, PathBuf, PathBuf, &'static str) {
    let task_home_path = workspace.join("task-home");
    let repository = workspace.join("repository");
    let fixture = workspace.join("archive-helper");
    fs::create_dir_all(&repository).unwrap();
    fs::create_dir_all(&fixture).unwrap();
    initialize_project(&repository);
    fs::write(
        fixture.join("package.json"),
        r#"{"name":"archive-helper","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(
        fixture.join("index.js"),
        "module.exports = 'remote-ready';\n",
    )
    .unwrap();
    let global_config = workspace.join("npm-globalrc");
    fs::write(&global_config, []).unwrap();
    let npm_home = workspace.join("npm-home");
    fs::create_dir_all(&npm_home).unwrap();
    let packed = Command::new("npm")
        .args([
            &format!("--globalconfig={}", global_config.display()),
            "--userconfig=/dev/null",
            "--offline",
            "--ignore-scripts",
            "pack",
            ".",
        ])
        .arg(format!("--pack-destination={}", workspace.display()))
        .current_dir(&fixture)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &npm_home)
        .output()
        .unwrap();
    assert!(
        packed.status.success(),
        "npm fixture pack failed: {}",
        String::from_utf8_lossy(&packed.stderr)
    );
    let archive = workspace.join("archive-helper-1.0.0.tgz");
    let integrity = format!(
        "sha512-{}",
        support::base64(&Sha512::digest(fs::read(&archive).unwrap()))
    );
    let registry_url = "https://registry.npmjs.org/archive-helper/-/archive-helper-1.0.0.tgz";
    fs::write(
        repository.join("package.json"),
        r#"{"name":"synthetic-task","version":"1.0.0","private":true,"dependencies":{"archive-helper":"1.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        repository.join("package-lock.json"),
        serde_json::to_vec(&serde_json::json!({
            "name":"synthetic-task", "version":"1.0.0", "lockfileVersion":3, "requires":true,
            "packages":{"":{"name":"synthetic-task","version":"1.0.0","dependencies":{"archive-helper":"1.0.0"}},
            "node_modules/archive-helper":{"version":"1.0.0","resolved":registry_url,"integrity":integrity}}
        }))
        .unwrap(),
    )
    .unwrap();
    commit(&repository);
    (task_home_path, repository, archive, registry_url)
}

pub(super) fn verify_authentication(
    sshd: &sshd::SshdFixture,
    worker_id: &str,
    worker: &crate::execution::remote::ssh::SshWorkerConfig,
) {
    let verified = worker.verify_known_host().unwrap();
    let mut wrong_key = worker.clone();
    wrong_key.verified_host_key_fingerprint = format!("SHA256:{}", "A".repeat(43));
    assert!(wrong_key.verify_known_host().is_err());
    sshd.write_wrong_known_host(worker_id).unwrap();
    let host_key_failure = sshd.run_worker_command(&verified);
    assert!(!host_key_failure.status.success());
    assert!(
        String::from_utf8_lossy(&host_key_failure.stderr).contains("Host key verification failed")
    );
    sshd.restore_known_host(worker_id).unwrap();
    let auth_failure = sshd.run_worker_command(&sshd.unauthenticated_worker(worker));
    assert!(!auth_failure.status.success());
    assert!(String::from_utf8_lossy(&auth_failure.stderr).contains("Permission denied"));
}
