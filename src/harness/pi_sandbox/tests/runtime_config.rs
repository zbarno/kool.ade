use super::support::{TestTree, bwrap_available, create_task_clone, git, run};
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
    sync::{Mutex, MutexGuard},
};

static STATE_HOME_LOCK: Mutex<()> = Mutex::new(());

struct StateHomeGuard {
    _lock: MutexGuard<'static, ()>,
    previous: Option<std::ffi::OsString>,
}

impl StateHomeGuard {
    fn set(path: &Path) -> Self {
        let lock = STATE_HOME_LOCK.lock().unwrap();
        let previous = std::env::var_os("KOOLADE_HOME");
        // This test owns the process-global state root until the guard drops.
        unsafe { std::env::set_var("KOOLADE_HOME", path) };
        Self {
            _lock: lock,
            previous,
        }
    }
}

impl Drop for StateHomeGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(previous) => unsafe { std::env::set_var("KOOLADE_HOME", previous) },
            None => unsafe { std::env::remove_var("KOOLADE_HOME") },
        }
    }
}

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
    let (repo, root) = create_task_clone(&tree, name);
    let remote = format!("https://example.invalid/{name}.git");
    git(&repo, &["remote", "add", "origin", &remote]);
    git(&root, &["remote", "set-url", "origin", &remote]);
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
        let sandbox = Sandbox::new_for_task_repository(&root, &repo).unwrap();
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
        assert!(
            runtime_config::paths_with_source(&root, Some(&repo)).is_err(),
            "{path}"
        );
    }
    grant(&repo, vec!["App/.env".into(), "App/.env".into()]);
    assert!(runtime_config::paths_with_source(&root, Some(&repo)).is_err());
    grant(&repo, vec!["App/.env".into()]);
    fs::write(root.join("App/.env"), "task-sentinel").unwrap();
    git(&root, &["add", "-f", "App/.env"]);
    assert!(runtime_config::paths_with_source(&root, Some(&repo)).is_err());
    git(&root, &["reset", "--", "App/.env"]);
    fs::remove_file(root.join("App/.env")).unwrap();
    std::os::unix::fs::symlink(repo.join("App/.env"), root.join("App/.env")).unwrap();
    assert!(runtime_config::paths_with_source(&root, Some(&repo)).is_err());
    fs::remove_file(root.join("App/.env")).unwrap();
    fs::remove_file(repo.join("App/.env")).unwrap();
    assert!(runtime_config::paths_with_source(&root, Some(&repo)).is_err());
    let manifest = repo.join(".git").join(runtime_config::GRANT_FILE);
    fs::remove_file(&manifest).unwrap();
    std::os::unix::fs::symlink("missing.json", &manifest).unwrap();
    assert!(runtime_config::paths_with_source(&root, Some(&repo)).is_err());
}

#[test]
fn runtime_config_blocks_direct_worker_resource_egress() {
    let (_tree, repo, root) = fixture("runtime-resource");
    let (progress, updates) = std::sync::mpsc::channel();
    let bridge = ResourceBridge::start_for_task_repository(
        &root,
        Some(&repo),
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

mod dependency;

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

#[test]
fn independent_clone_uses_source_checkout_grants_and_blocks_resource_egress() {
    let (tree, repo, _) = fixture("runtime-clone-source");
    let home = tree.0.join("koolade-home");
    fs::create_dir_all(&home).unwrap();
    let _state_home = StateHomeGuard::set(&home);

    fs::write(repo.join("App/.keep"), "fixture\n").unwrap();
    git(&repo, &["add", ".gitignore", "App/.keep"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Koolade test",
            "-c",
            "user.email=koolade-test@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "add runtime config fixture",
        ],
    );
    git(
        &repo,
        &[
            "remote",
            "set-url",
            "origin",
            "https://example.invalid/fixture.git",
        ],
    );
    let project_id = crate::persistence::project_slug(&repo.canonicalize().unwrap());
    let task_repository = home
        .join("projects")
        .join(project_id)
        .join("task-repositories")
        .join("repository-1")
        .join("task-0123456789abcdef");
    fs::create_dir_all(task_repository.parent().unwrap()).unwrap();
    git(
        &repo,
        &[
            "clone",
            "--quiet",
            repo.to_str().unwrap(),
            task_repository.to_str().unwrap(),
        ],
    );
    git(
        &task_repository,
        &[
            "remote",
            "set-url",
            "origin",
            "https://example.invalid/fixture.git",
        ],
    );

    assert!(task_repository.join(".git").is_dir());
    assert_eq!(
        runtime_config::paths_with_source(&task_repository, Some(&repo)).unwrap(),
        ["App/.env".to_owned()].into()
    );

    let bridge = ResourceBridge::start_with_runtime_source(&task_repository, Some(&repo)).unwrap();
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
    assert!(response.contains("downloads are disabled"));
    assert!(!response.contains("synthetic-private-sentinel"));

    if bwrap_available() {
        let sandbox = Sandbox::new_for_task_repository(&task_repository, &repo).unwrap();
        let output = run(
            &sandbox,
            "/bin/bash",
            "test \"$(cat App/.env)\" = SYNTHETIC_CONFIG=approved-sentinel",
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
