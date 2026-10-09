use std::process::Command;

use super::support::{TestTree, bwrap_available, create_task_clone, run};
use crate::harness::pi_sandbox::{Sandbox, components};

#[test]
fn sandbox_reuses_nuget_toolchains_and_keeps_npm_cache_project_scoped() {
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
    let (_repository, root) = create_task_clone(&tree, "host-components");

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
        assert!(sandbox.args.windows(3).any(|mount| {
            mount[0] == "--ro-bind" && mount[2] == "/tmp/koolade-home/.nuget/http-cache"
        }));
        assert!(sandbox.args.windows(3).any(|env| {
            env[0] == "--setenv"
                && env[1] == "NUGET_HTTP_CACHE_PATH"
                && env[2] == "/tmp/koolade-home/.nuget/http-cache"
        }));
        for (key, expected) in [
            ("npm_config_offline", "true"),
            ("npm_config_audit", "false"),
            ("npm_config_globalconfig", "/tmp/koolade-home/.npm-globalrc"),
        ] {
            assert!(
                sandbox
                    .args
                    .windows(3)
                    .any(|env| { env[0] == "--setenv" && env[1] == key && env[2] == expected })
            );
        }
        assert!(!sandbox.args.windows(3).any(|mount| {
            mount[0] == "--ro-bind" && mount[2] == "/tmp/koolade-home/.npm/_cacache"
        }));
        assert!(
            !sandbox
                .args
                .windows(3)
                .any(|env| { env[0] == "--setenv" && env[1] == "npm_config_cache" })
        );
        let npm = run(
            &sandbox,
            "/bin/sh",
            "test -z \"${npm_config_cache+x}\" && \
             test \"$npm_config_globalconfig\" = /tmp/koolade-home/.npm-globalrc && \
             test -f \"$npm_config_globalconfig\"",
        );
        assert!(
            npm.status.success(),
            "sandbox npm configuration was not isolated: {}",
            String::from_utf8_lossy(&npm.stderr)
        );

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

#[test]
fn cargo_registry_keeps_archives_read_only_and_source_extraction_ephemeral() {
    if !cfg!(target_os = "linux") || !bwrap_available() {
        return;
    }
    let tree = TestTree::new();
    let (_repository, root) = create_task_clone(&tree, "cargo-source-cache");
    let cache = tree.0.join("prepared-cargo-cache");
    let archive = cache.join("registry/cache/index.crates.io-test/serde-1.0.0.crate");
    std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
    std::fs::create_dir_all(cache.join("registry/index")).unwrap();
    std::fs::create_dir_all(cache.join("registry/src")).unwrap();
    std::fs::write(&archive, "verified crate archive").unwrap();

    let mut sandbox = Sandbox::new(&root).unwrap();
    sandbox.mount_cargo_cache(&cache).unwrap();
    let output = run(
        &sandbox,
        "/bin/sh",
        "test -f /tmp/koolade-tools/cargo-home/registry/cache/index.crates.io-test/serde-1.0.0.crate && \
         mkdir -p /tmp/koolade-tools/cargo-home/registry/src/index.crates.io-test/serde-1.0.0/src && \
         touch /tmp/koolade-tools/cargo-home/registry/src/index.crates.io-test/serde-1.0.0/src/lib.rs && \
         if touch /tmp/koolade-tools/cargo-home/registry/cache/index.crates.io-test/write-guard 2>/dev/null; then exit 21; fi",
    );
    assert!(
        output.status.success(),
        "Cargo archive/source cache permissions were unsafe: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !cache
            .join("registry/src/index.crates.io-test/serde-1.0.0/src/lib.rs")
            .exists(),
        "sandbox source extraction escaped its temporary mount"
    );
}
