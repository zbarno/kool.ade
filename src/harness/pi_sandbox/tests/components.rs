use std::process::Command;

use super::support::{TestTree, bwrap_available, create_worktree, run};
use crate::harness::pi_sandbox::{Sandbox, components};

#[test]
fn sandbox_reuses_host_toolchains_and_package_caches_across_invocations() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }

    let host_packages = components::host_nuget_packages().unwrap();
    let host_npm_cache = components::host_npm_cache().unwrap();
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
        for (key, expected) in [
            ("npm_config_offline", "true"),
            ("npm_config_audit", "false"),
        ] {
            assert!(
                sandbox
                    .args
                    .windows(3)
                    .any(|env| { env[0] == "--setenv" && env[1] == key && env[2] == expected })
            );
        }
        if let Some(host_cache) = &host_npm_cache {
            assert!(sandbox.args.windows(3).any(|mount| {
                mount[0] == "--ro-bind"
                    && mount[1] == host_cache.to_string_lossy()
                    && mount[2] == "/tmp/koolade-home/.npm/_cacache"
            }));
            assert!(sandbox.args.windows(3).any(|env| {
                env[0] == "--setenv"
                    && env[1] == "npm_config_cache"
                    && env[2] == "/tmp/koolade-home/.npm"
            }));
            let npm = run(
                &sandbox,
                "/bin/sh",
                "test \"$npm_config_cache\" = /tmp/koolade-home/.npm && \
                 test -d \"$npm_config_cache/_cacache\" && \
                 if touch \"$npm_config_cache/_cacache/.koolade-write-guard\" 2>/dev/null; then exit 22; fi",
            );
            assert!(
                npm.status.success(),
                "npm cache was not available read-only: {}",
                String::from_utf8_lossy(&npm.stderr)
            );
        }

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
