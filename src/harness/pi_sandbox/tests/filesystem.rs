use std::{fs, net::TcpListener, path::Path};

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
        sandbox
            .args
            .windows(3)
            .any(|args| args == ["--ro-bind", "/", "/"])
    );
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
