use super::{fixture, runtime_config};
use crate::harness::DependencyRequestStatus;
use sha2::{Digest, Sha512};
use std::{
    fs,
    io::{Read, Write},
    os::unix::net::UnixStream,
    process::Command,
    sync::{Arc, mpsc},
    time::Duration,
};

#[test]
fn private_runtime_config_blocks_managed_npm_fetch_without_a_verified_cache() {
    if !Command::new("npm")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let (tree, repo, root) = fixture("runtime-dependency");
    assert!(
        !runtime_config::paths_with_source(&root, Some(&repo))
            .unwrap()
            .is_empty()
    );
    let package_dir = tree.0.join("synthetic-runtime-helper");
    fs::create_dir_all(&package_dir).unwrap();
    fs::write(
        package_dir.join("package.json"),
        r#"{"name":"synthetic-runtime-helper","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    fs::write(package_dir.join("index.js"), "module.exports = true;\n").unwrap();
    let npm_home = tree.0.join("npm-home");
    fs::create_dir_all(&npm_home).unwrap();
    let global_config = tree.0.join("npm-globalrc");
    fs::write(&global_config, []).unwrap();
    let packed = Command::new("npm")
        .args([
            &format!("--globalconfig={}", global_config.display()),
            "--userconfig=/dev/null",
            "--offline",
            "--ignore-scripts",
            "pack",
            ".",
        ])
        .arg(format!("--pack-destination={}", tree.0.display()))
        .current_dir(&package_dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", &npm_home)
        .output()
        .unwrap();
    assert!(
        packed.status.success(),
        "synthetic npm package setup failed: {}",
        String::from_utf8_lossy(&packed.stderr)
    );
    let archive = tree.0.join("synthetic-runtime-helper-1.0.0.tgz");
    let integrity = format!(
        "sha512-{}",
        base64(&Sha512::digest(fs::read(&archive).unwrap()))
    );
    let registry = "https://registry.npmjs.org/";
    let archive_url =
        format!("{registry}synthetic-runtime-helper/-/synthetic-runtime-helper-1.0.0.tgz");
    fs::write(
        root.join("package.json"),
        r#"{"name":"synthetic-task","version":"1.0.0","dependencies":{"synthetic-runtime-helper":"1.0.0"}}"#,
    )
    .unwrap();
    fs::write(
        root.join("package-lock.json"),
        serde_json::to_vec(&serde_json::json!({
            "name": "synthetic-task",
            "version": "1.0.0",
            "lockfileVersion": 3,
            "packages": {
                "": {
                    "name": "synthetic-task",
                    "version": "1.0.0",
                    "dependencies": { "synthetic-runtime-helper": "1.0.0" },
                },
                "node_modules/synthetic-runtime-helper": {
                    "version": "1.0.0",
                    "resolved": archive_url,
                    "integrity": integrity,
                },
            },
        }))
        .unwrap(),
    )
    .unwrap();

    let state_root = tree.0.join("app-state");
    fs::create_dir_all(&state_root).unwrap();
    let (progress, updates) = mpsc::channel();
    let bridge = crate::harness::resource_bridge::start_with_test_npm_preparation(
        crate::harness::resource_bridge::TestNpmPreparation {
            worktree: &root,
            runtime_source: Some(&repo),
            task_id: Some("synthetic-task-uid"),
            progress,
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            state_root: &state_root,
            archive,
            registry_url: registry.into(),
        },
    )
    .unwrap();
    let mut stream = UnixStream::connect(bridge.socket_path()).unwrap();
    let request = serde_json::json!({
        "action": "dependency_request",
        "dependency": {
            "ecosystem": "npm",
            "source": registry,
            "command": "npm ci --no-audit",
            "reason": "Restore the locked public package required by this task",
            "kind": "existing_restore"
        },
        "purpose": "synthetic fixture"
    });
    stream
        .write_all(format!("{}\n", request).as_bytes())
        .unwrap();
    let (response_tx, response_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut response = String::new();
        let result = stream.read_to_string(&mut response).map(|_| response);
        let _ = response_tx.send(result);
    });

    let reviewed = loop {
        let update = updates
            .recv_timeout(Duration::from_secs(5))
            .expect("Man.ager should receive the locked npm dependency request");
        if let Some(request) = update.dependency_requests.into_iter().next()
            && request.status == DependencyRequestStatus::ManagerReviewing
        {
            break request;
        }
    };
    assert!(crate::harness::dependency_authorization::answer(
        &reviewed.id,
        &reviewed.task_id,
        &reviewed.need,
        crate::harness::dependency_authorization::DependencyResolution {
            decision: crate::harness::DependencyDecision::AutoAuthorize,
            scope: None,
            rationale: "The package is lockfile-pinned to the public npm registry.".into(),
        }
    ));

    let response = response_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("managed package request should finish with private runtime config")
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(parsed["status"], "needs_attention");
    assert_eq!(parsed["dependency_request"]["status"], "failed");
    assert_eq!(parsed["dependency_result"]["bytesDownloaded"], 0);
    assert!(
        parsed["summary"]
            .as_str()
            .unwrap()
            .contains("Fresh npm package downloads are disabled")
    );
    assert!(!response.contains("SYNTHETIC_CONFIG=approved-sentinel"));
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(a >> 2) as usize] as char);
        output.push(TABLE[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}
