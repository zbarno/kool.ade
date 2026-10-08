use std::{fs, net::TcpListener, path::Path, process::Command};

use crate::harness::pi_sandbox::Sandbox;

use super::support::{TestTree, bwrap_available, create_worktree, run};

#[test]
fn implementation_boundary_writes_only_to_its_worktree_and_hides_host_state() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    let tree = TestTree::new();
    let (_repository, root) = create_worktree(&tree, "write-boundary");
    let outside = tree.0.join("outside.txt");
    let sandbox = Sandbox::new(&root).unwrap();
    for required in ["/home", "/root", "/mnt", "/run", "/tmp", "/boot", "/sys"] {
        if Path::new(required).is_dir() {
            assert!(
                sandbox
                    .args
                    .windows(2)
                    .any(|pair| pair == ["--tmpfs", required])
            );
        }
    }
    assert!(
        !sandbox
            .args
            .windows(3)
            .any(|args| args == ["--ro-bind", "/", "/"]),
        "implementation must never bind the host root"
    );
    // A compiler alternative may be required to resolve /usr/bin/cc, but
    // the surrounding /etc/alternatives directory must remain invisible.
    if let Ok(compiler) = Path::new("/etc/alternatives/cc").canonicalize()
        && compiler.is_file()
        && compiler.starts_with("/usr")
    {
        assert!(
            sandbox.args.windows(3).any(|args| {
                args[0] == "--ro-bind"
                    && args[1] == compiler.to_string_lossy()
                    && args[2] == "/etc/alternatives/cc"
            }),
            "missing narrowly mounted compiler alias"
        );
        assert!(
            !sandbox
                .args
                .windows(3)
                .any(|args| { args[0] == "--ro-bind" && args[1] == "/etc/alternatives" }),
            "do not expose the entire alternatives directory"
        );
    }
    for allowed in ["/usr", "/etc/ssl/certs"] {
        if Path::new(allowed).exists() {
            assert!(
                sandbox.args.windows(3).any(|args| {
                    args[0] == "--ro-bind" && args[1] == allowed && args[2] == allowed
                }),
                "missing explicit read-only runtime mount: {allowed}"
            );
        }
    }
    assert!(
        sandbox
            .args
            .windows(2)
            .any(|args| args == ["--unshare-net", "--unshare-ipc"])
    );
    assert!(
        sandbox
            .args
            .windows(2)
            .any(|args| args == ["--unshare-pid", "--unshare-net"])
    );

    let host_pid = std::process::id();
    let host_home = std::env::var("HOME").unwrap_or_else(|_| "/home".into());
    let command = format!(
        "echo allowed > \"$KOOLADE_WORKTREE/inside.txt\"; \
         echo temporary > '{}'; \
         test ! -w /; \
         if echo forbidden >> /etc/passwd 2>/dev/null; then exit 42; fi; \
         test ! -e '{host_home}'; \
         test -z \"${{AWS_ACCESS_KEY_ID:-}}\"; \
         test ! -s /etc/shadow; \
         if kill -0 {host_pid} 2>/dev/null; then exit 43; fi",
        outside.display()
    );
    let output = run(&sandbox, "/bin/bash", &command);
    assert!(
        output.status.success(),
        "sandbox rejected its own test unexpectedly: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.join("inside.txt")).unwrap(),
        "allowed\n"
    );
    assert!(
        !outside.exists(),
        "sandbox wrote outside its worktree to the host filesystem"
    );
}

#[test]
fn implementation_boundary_has_no_host_network_route() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let tree = TestTree::new();
    let (_repository, root) = create_worktree(&tree, "network-boundary");
    let sandbox = Sandbox::new(&root).unwrap();
    let command = format!(
        "if timeout 2 /bin/bash -c 'echo blocked > /dev/tcp/127.0.0.1/{port}' 2>/dev/null; then exit 44; fi; echo isolated > network-check"
    );
    let output = run(&sandbox, "/bin/bash", &command);
    drop(listener);
    assert!(output.status.success(), "sandbox allowed host networking");
    assert_eq!(
        fs::read_to_string(root.join("network-check")).unwrap(),
        "isolated\n"
    );
}

#[test]
fn npm_install_lifecycle_stays_offline_with_prepared_cache_mounted() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    if !Command::new("npm")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let tree = TestTree::new();
    let (_repository, root) = create_worktree(&tree, "npm-lifecycle-boundary");
    fs::write(
        root.join("package.json"),
        r#"{"name":"koolade-offline-fixture","version":"1.0.0","private":true}"#,
    )
    .unwrap();
    let dependency = root.join("fixtures/lifecycle-dependency");
    fs::create_dir_all(&dependency).unwrap();
    let lifecycle = format!(
        "node -e \"const fs=require('fs'),net=require('net');const out=process.env.INIT_CWD+'/lifecycle-network';const socket=net.connect({port},'127.0.0.1');const timer=setTimeout(()=>{{fs.writeFileSync(out,'blocked');process.exit(0)}},1500);socket.on('connect',()=>{{clearTimeout(timer);fs.writeFileSync(out,'connected');process.exit(95)}});socket.on('error',()=>{{clearTimeout(timer);fs.writeFileSync(out,'blocked')}});\""
    );
    fs::write(
        dependency.join("package.json"),
        serde_json::to_vec(&serde_json::json!({
            "name": "koolade-lifecycle-fixture",
            "version": "1.0.0",
            "scripts": { "preinstall": lifecycle },
        }))
        .unwrap(),
    )
    .unwrap();
    let prepared_cache = tree.0.join("prepared-npm-cache");
    fs::create_dir_all(prepared_cache.join("_cacache")).unwrap();
    let mut sandbox = Sandbox::new(&root).unwrap();
    sandbox.mount_npm_cache(&prepared_cache).unwrap();
    let output = run(
        &sandbox,
        "/bin/bash",
        "test -d /tmp/koolade-home/.npm-prepared/_cacache && npm install --offline --no-audit --no-fund ./fixtures/lifecycle-dependency",
    );
    drop(listener);
    assert!(
        output.status.success(),
        "offline npm retry failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.join("lifecycle-network")).unwrap(),
        "blocked"
    );
}

#[test]
fn sandboxed_verification_keeps_worktree_and_toolchain_inside_posix_shell() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    let tree = TestTree::new();
    let (_repository, root) = create_worktree(&tree, "worktree with spaces");
    fs::write(root.join("marker"), "proof").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"sandbox-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(
        root.join("src/lib.rs"),
        "#[test] fn runs_in_the_assigned_worktree() { assert_eq!(2 + 2, 4); }\n",
    )
    .unwrap();
    let sandbox = Sandbox::new(&root).unwrap();
    let output = run(
        &sandbox,
        "/bin/sh",
        "test \"$(cat \"$KOOLADE_WORKTREE/marker\")\" = proof && test \"$PWD\" = \"$KOOLADE_WORKTREE\" && cargo test --offline --quiet",
    );
    assert!(
        output.status.success(),
        "verification failed inside sandbox: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join("target/debug/deps").is_dir());
}

#[test]
fn implementation_boundary_cannot_read_unregistered_host_files_or_follow_escaping_symlinks() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    let tree = TestTree::new();
    let (_repository, root) = create_worktree(&tree, "read-boundary");
    let private = tree.0.join("operator-secret.txt");
    fs::write(&private, "operator-only-sentinel").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&private, root.join("escape-link")).unwrap();

    let sandbox = Sandbox::new(&root).unwrap();
    let command = format!(
        "test ! -e '{}' && \
         test ! -e '{}' && \
         test ! -e /etc/shadow && \
         test ! -e /opt/koolade-operator-secret && \
         test -r /etc/passwd && \
         test -x /bin/sh",
        private.display(),
        root.join("escape-link").display()
    );
    let output = run(&sandbox, "/bin/bash", &command);
    assert!(
        output.status.success(),
        "unauthorized host file became readable: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&private).unwrap(),
        "operator-only-sentinel"
    );
}
