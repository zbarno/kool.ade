use std::process::Command;

use super::support::{TestTree, bwrap_available, create_worktree, run};
use crate::harness::pi_sandbox::{Sandbox, components};

#[test]
fn sandbox_reuses_host_dotnet_and_nuget_cache_across_invocations() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }

    let host_packages = components::host_nuget_packages().unwrap();
    let host_dotnet = Command::new("dotnet")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let tree = TestTree::new();
    let (_repository, root) = create_worktree(&tree, "host-components");

    for _ in 0..2 {
        let sandbox = Sandbox::new(&root).unwrap();
        assert!(sandbox.args.windows(3).any(|mount| {
            mount[0] == "--ro-bind"
                && mount[1] == host_packages.to_string_lossy()
                && mount[2] == "/tmp/koolade-home/.nuget/packages"
        }));
        assert!(sandbox.args.windows(3).any(|env| {
            env[0] == "--setenv"
                && env[1] == "NUGET_PACKAGES"
                && env[2] == "/tmp/koolade-home/.nuget/packages"
        }));

        let cache = run(
            &sandbox,
            "/bin/sh",
            "test \"$NUGET_PACKAGES\" = /tmp/koolade-home/.nuget/packages && \
             test -d \"$NUGET_PACKAGES\" && \
             if touch \"$NUGET_PACKAGES/.koolade-write-guard\" 2>/dev/null; then exit 21; fi",
        );
        assert!(
            cache.status.success(),
            "NuGet cache was not available read-only: {}",
            String::from_utf8_lossy(&cache.stderr)
        );

        if let Some(host_version) = &host_dotnet {
            let guest = run(&sandbox, "/bin/sh", "dotnet --version");
            assert!(
                guest.status.success(),
                "host dotnet was unavailable in the sandbox: {}",
                String::from_utf8_lossy(&guest.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&guest.stdout).trim(), host_version);
        }
    }
}
