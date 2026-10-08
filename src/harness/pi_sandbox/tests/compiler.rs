use std::{fs, path::Path};

use crate::harness::pi_sandbox::Sandbox;

use super::support::{TestTree, bwrap_available, create_worktree, run};

#[test]
fn compiler_alternative_retains_support_file_discovery() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    // This regression concerns Debian-style compiler alternatives. Other
    // layouts remain covered by the Rust compilation verification fixture.
    let Ok(compiler) = Path::new("/etc/alternatives/cc").canonicalize() else {
        return;
    };
    if !compiler.is_file() || !compiler.starts_with("/usr") {
        return;
    }
    let tree = TestTree::new();
    let (_repository, root) = create_worktree(&tree, "compiler-runtime");
    fs::write(root.join("fixture.c"), "int main(void) { return 0; }\n").unwrap();
    let sandbox = Sandbox::new(&root).unwrap();
    let output = run(
        &sandbox,
        "/bin/sh",
        "set -eu; test -L /etc/alternatives/cc; \
         cc fixture.c -o fixture; ./fixture; \
         test ! -e /etc/alternatives/editor; test ! -e /etc/shadow",
    );
    assert!(
        output.status.success(),
        "compiler could not discover its runtime in the sandbox: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
