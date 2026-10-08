use super::support::{TestTree, bwrap_available, create_worktree, git, run};
use crate::harness::{
    pi_sandbox::{
        Sandbox,
        runtime_config::{self, Grant},
    },
    resource_bridge::ResourceBridge,
};
use std::{
    fs,
    io::{Read, Write},
    net::Shutdown,
    os::unix::{fs::PermissionsExt, net::UnixStream},
    path::Path,
};

fn grant(repo: &Path, files: Vec<String>) {
    let dir = repo.join(".git/koolade");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let path = repo.join(".git").join(runtime_config::GRANT_FILE);
    fs::write(
        &path,
        serde_json::to_vec(&Grant {
            schema_version: 1,
            source_root: repo.canonicalize().unwrap(),
            files,
        })
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

fn fixture(name: &str) -> (TestTree, std::path::PathBuf, std::path::PathBuf) {
    let tree = TestTree::new();
    let (repo, root) = create_worktree(&tree, name);
    for checkout in [&repo, &root] {
        fs::write(checkout.join(".gitignore"), ".env\n").unwrap();
        fs::create_dir_all(checkout.join("App")).unwrap();
    }
    fs::write(
        repo.join("App/.env"),
        "SYNTHETIC_CONFIG=approved-sentinel\n",
    )
    .unwrap();
    fs::set_permissions(repo.join("App/.env"), fs::Permissions::from_mode(0o600)).unwrap();
    grant(&repo, vec!["App/.env".into()]);
    (tree, repo, root)
}

#[test]
fn runtime_config_mount_uses_source_readonly_without_copying_or_host_access() {
    if !bwrap_available() {
        return;
    }
    for existing in [false, true] {
        let (_tree, repo, root) = fixture("runtime-mount");
        if existing {
            fs::write(root.join("App/.env"), "old-task-config\n").unwrap();
        }
        fs::write(repo.join("unapproved.txt"), "unapproved-host-secret").unwrap();
        let sandbox = Sandbox::new(&root).unwrap();
        let command = format!(
            "test \"$(cat App/.env)\" = SYNTHETIC_CONFIG=approved-sentinel && ! echo changed >> App/.env && test ! -e '{}' && test -z \"${{AWS_ACCESS_KEY_ID:-}}\"",
            repo.join("unapproved.txt").display()
        );
        let output = run(&sandbox, "/bin/bash", &command);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(repo.join("App/.env")).unwrap(),
            "SYNTHETIC_CONFIG=approved-sentinel\n"
        );
        let task = fs::read_to_string(root.join("App/.env")).unwrap_or_default();
        assert_eq!(task, if existing { "old-task-config\n" } else { "" });
        assert!(git(&root, &["ls-files", "App/.env"]).is_empty());
    }
}

#[test]
fn runtime_config_rejects_untrusted_grants_and_paths() {
    let (_tree, repo, root) = fixture("runtime-invalid");
    for path in [
        "../App/.env",
        "/tmp/.env",
        "App/config.json",
        "App/.env/../.env",
    ] {
        grant(&repo, vec![path.into()]);
        assert!(runtime_config::paths(&root).is_err(), "{path}");
    }
    grant(&repo, vec!["App/.env".into(), "App/.env".into()]);
    assert!(runtime_config::paths(&root).is_err());
    grant(&repo, vec!["App/.env".into()]);
    fs::write(root.join("App/.env"), "task-sentinel").unwrap();
    git(&root, &["add", "-f", "App/.env"]);
    assert!(runtime_config::paths(&root).is_err());
    git(&root, &["reset", "--", "App/.env"]);
    fs::remove_file(root.join("App/.env")).unwrap();
    std::os::unix::fs::symlink(repo.join("App/.env"), root.join("App/.env")).unwrap();
    assert!(runtime_config::paths(&root).is_err());
    fs::remove_file(root.join("App/.env")).unwrap();
    fs::remove_file(repo.join("App/.env")).unwrap();
    assert!(runtime_config::paths(&root).is_err());
    let manifest = repo.join(".git").join(runtime_config::GRANT_FILE);
    fs::remove_file(&manifest).unwrap();
    std::os::unix::fs::symlink("missing.json", &manifest).unwrap();
    assert!(runtime_config::paths(&root).is_err());
}

#[test]
fn runtime_config_blocks_direct_worker_resource_egress() {
    let (_tree, _repo, root) = fixture("runtime-resource");
    let (progress, updates) = std::sync::mpsc::channel();
    let bridge = ResourceBridge::start(
        &root,
        None,
        progress,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .unwrap();
    let mut stream = UnixStream::connect(bridge.socket_path()).unwrap();
    let request = serde_json::json!({
        "action":"fetch",
        "url":"https://registry.npmjs.org/synthetic-private-sentinel",
        "purpose":"synthetic fixture"
    });
    stream.write_all(request.to_string().as_bytes()).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(parsed["status"], "needs_attention");
    assert!(
        updates
            .try_iter()
            .all(|update| update.dependency_requests.is_empty())
    );
}

#[test]
fn private_runtime_config_does_not_block_managed_locked_dependency_restore() {
    let (_tree, _repo, root) = fixture("runtime-dependency");
    assert!(!runtime_config::paths(&root).unwrap().is_empty());
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"synthetic-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(root.join("Cargo.lock"), "version = 4\n").unwrap();
    let (progress, updates) = std::sync::mpsc::channel();
    let bridge = ResourceBridge::start(
        &root,
        Some("synthetic-task-uid"),
        progress,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    )
    .unwrap();
    let mut stream = UnixStream::connect(bridge.socket_path()).unwrap();
    let request = serde_json::json!({
        "action":"dependency_request",
        "dependency":{
            "ecosystem":"cargo",
            "source":"https://index.crates.io",
            "command":"cargo fetch --locked",
            "reason":"Restore the declared dependencies for this task",
            "kind":"existing_restore"
        },
        "purpose":"synthetic fixture"
    });
    stream.write_all(request.to_string().as_bytes()).unwrap();
    stream.shutdown(Shutdown::Write).unwrap();
    let (response_tx, response_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut response = String::new();
        let result = stream.read_to_string(&mut response).map(|_| response);
        let _ = response_tx.send(result);
    });

    let reviewed = loop {
        let update = updates
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("Man.ager should receive the structured dependency request");
        if let Some(request) = update.dependency_requests.into_iter().next()
            && request.status == crate::harness::DependencyRequestStatus::ManagerReviewing
        {
            break request;
        }
    };
    assert!(crate::harness::dependency_authorization::answer(
        &reviewed.id,
        crate::harness::dependency_authorization::DependencyResolution {
            decision: crate::harness::DependencyDecision::AutoAuthorize,
            scope: None,
            rationale: "The empty lockfile fixture requires no external package.".into(),
        }
    ));

    let response = response_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the mediated restore should finish without a network fetch")
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(parsed["status"], "prepared");
    assert!(!response.contains("SYNTHETIC_CONFIG=approved-sentinel"));
}

#[test]
fn new_npm_dependency_waits_for_broker_preparation_then_retries_offline_once() {
    let extension = include_str!("../restricted_bash.ts");
    assert!(extension.contains("from \"./dependency_retry.mjs\""));
    assert!(extension.contains("withPreparedNpmCacheIndex"));
    assert!(extension.contains("from \"./dependency_operation.mjs\""));
    let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src/harness/pi_sandbox/dependency_retry.mjs");
    let module = url::Url::from_file_path(helper).unwrap().to_string();
    let operation_module = url::Url::from_file_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/harness/pi_sandbox/dependency_operation.mjs"),
    )
    .unwrap()
    .to_string();
    let script = format!(
        r#"
import assert from "node:assert/strict";
import {{ disableShellGlobbing, prepareThenRetry, withPreparedNpmCacheIndex }} from {module:?};
import {{ dependencyOperation }} from {operation_module:?};

const order = [];
const prepared = await prepareThenRetry(
  async () => {{ order.push("broker"); return {{ status: "prepared", summary: "cache ready" }}; }},
  async () => {{ order.push("offline retry"); return {{ text: "install complete", details: {{ exitCode: 0 }}, isError: false }}; }},
);
assert.deepEqual(order, ["broker", "offline retry"]);
assert.equal(prepared.retry.text, "install complete");
assert.equal(prepared.isError, false);
const staged = withPreparedNpmCacheIndex("set -f; npm ci");
assert.ok(staged.indexOf("index-source-v5") < staged.indexOf("npm ci"));
assert.ok(staged.indexOf("cp -R") < staged.indexOf("npm ci"));

let retryCount = 0;
const denied = await prepareThenRetry(
  async () => ({{ status: "authorization_required", summary: "approval required" }}),
  async () => {{ retryCount += 1; return {{ text: "unexpected", details: {{}}, isError: false }}; }},
);
assert.equal(retryCount, 0);
assert.equal(denied.isError, true);

const need = dependencyOperation("npm install zod@^4.0.0 --registry=https://packages.example.net/");
assert.equal(need.ecosystem, "npm");
assert.equal(need.package, "zod");
assert.equal(need.source, "https://packages.example.net/");
const restore = dependencyOperation("npm ci --no-audit --registry=https://packages.example.net/");
assert.equal(restore.kind, "existing_restore");
assert.equal(restore.source, "https://packages.example.net/");

const paddedCommand = `npm install zod@^4.0.0${{" ".repeat(2048)}}; npm install left-pad@1.0.0`;
const paddedNeed = dependencyOperation(paddedCommand);
assert.equal(paddedNeed.command, paddedCommand);
const fixture = await import("node:fs/promises");
const directory = await fixture.mkdtemp("/tmp/koolade-glob-command-");
await fixture.writeFile(`${{directory}}/zod@4.0.0`, "matching filename");
const shell = await import("node:child_process");
const glob = shell.spawnSync("/bin/bash", ["-c", disableShellGlobbing("printf '%s\\n' zod@*")], {{ cwd: directory, encoding: "utf8" }});
assert.equal(glob.status, 0);
assert.equal(glob.stdout, "zod@*\n");
await fixture.rm(directory, {{ recursive: true }});
let rejectedRetryCount = 0;
await prepareThenRetry(
  async () => ({{ status: "rejected", summary: "command too long" }}),
  async () => {{ rejectedRetryCount += 1; return {{ text: "unexpected", details: {{}}, isError: false }}; }},
);
assert.equal(rejectedRetryCount, 0);
"#
    );
    let output = std::process::Command::new("node")
        .args(["--input-type=module", "-e", &script])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Node retry-flow test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
