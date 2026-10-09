use super::*;

struct TestTree(PathBuf);

impl TestTree {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("koolade-runtime-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn runtime_view_omits_host_data_parents_and_preserves_build_runtime() {
    let mut args = Vec::new();
    let mut created = BTreeSet::from(["/".to_owned()]);
    mount_system_runtime(&mut args, &mut created).unwrap();
    for mount in args.windows(3).filter(|args| args[0] == "--ro-bind") {
        for parent in [
            "/",
            "/usr",
            "/usr/local",
            "/usr/share",
            "/etc",
            "/opt",
            "/var",
        ] {
            assert_ne!(mount[1], parent, "broad runtime source");
            assert_ne!(mount[2], parent, "broad runtime destination");
        }
    }
    for directory in ["/usr/bin", "/usr/lib", "/usr/include"] {
        if Path::new(directory).is_dir() {
            assert!(
                args.windows(3)
                    .any(|mount| mount[0] == "--ro-bind" && mount[2] == directory)
            );
        }
    }
    for private in [
        "/usr/local/src",
        "/usr/local/etc",
        "/usr/local/var",
        "/usr/share/doc",
        "/opt",
        "/etc/ssl/private",
    ] {
        assert!(!runtime_visible(Path::new(private)));
        assert!(
            !args
                .windows(3)
                .any(|mount| mount[0] == "--ro-bind" && mount[2] == private)
        );
    }
}

#[test]
fn tool_alternatives_preserve_canonical_targets_without_exposing_other_aliases() {
    let mut args = Vec::new();
    let mut created = BTreeSet::from(["/".to_owned()]);
    mount_system_runtime(&mut args, &mut created).unwrap();
    for tool in ["cc", "c++", "cpp", "automake", "aclocal"] {
        let alias = Path::new("/etc/alternatives").join(tool);
        if let Ok(target) = alias.canonicalize() {
            assert!(
                args.windows(3).any(|mount| {
                    mount[0] == "--symlink"
                        && mount[1] == target.to_string_lossy()
                        && mount[2] == alias.to_string_lossy()
                }),
                "missing canonical tool alternative {tool}"
            );
        }
    }
    assert!(
        !args
            .windows(3)
            .any(|mount| { mount[0] == "--ro-bind" && mount[2] == "/etc/alternatives" })
    );
    assert!(
        !args
            .windows(3)
            .any(|mount| { mount[0] == "--symlink" && mount[2] == "/etc/alternatives/editor" })
    );
}

#[test]
fn runtime_view_mounts_fedora_os_files_without_their_parent_trees() {
    let mut args = Vec::new();
    let mut created = BTreeSet::from(["/".to_owned()]);
    mount_system_runtime(&mut args, &mut created).unwrap();

    let authselect = Path::new("/etc/authselect/nsswitch.conf");
    let fedora = Path::new("/etc/fedora-release").is_file();
    if fedora {
        assert!(
            authselect.is_file(),
            "Fedora authselect nsswitch target is missing"
        );
    }
    if authselect.is_file() {
        assert!(args.windows(3).any(|mount| {
            mount[0] == "--ro-bind"
                && mount[1] == authselect.to_string_lossy()
                && mount[2] == "/etc/nsswitch.conf"
        }));
    }
    let fedora_ca_roots = [
        "/etc/pki/tls/certs",
        "/etc/pki/ca-trust/extracted/pem",
        "/etc/pki/ca-trust/extracted/openssl",
    ];
    for location in fedora_ca_roots {
        let path = Path::new(location);
        if fedora {
            assert!(
                path.is_dir(),
                "Fedora CA runtime path is missing: {location}"
            );
        }
        if path.is_dir() {
            let source = path.canonicalize().unwrap();
            assert!(args.windows(3).any(|mount| {
                mount[0] == "--ro-bind"
                    && mount[1] == source.to_string_lossy()
                    && mount[2] == location
            }));
        }
    }
    for parent in ["/etc/authselect", "/etc/pki", "/etc/pki/ca-trust"] {
        assert!(!args.windows(3).any(|mount| {
            mount[0] == "--ro-bind" && (mount[1] == parent || mount[2] == parent)
        }));
    }
}

#[cfg(unix)]
#[test]
fn runtime_symlinks_cannot_redirect_into_private_or_parent_directories() {
    let tree = TestTree::new();
    let trusted = tree.0.join("runtime");
    let private = tree.0.join("simulated-opt/secrets");
    fs::create_dir_all(&trusted).unwrap();
    fs::create_dir_all(&private).unwrap();
    fs::write(private.join("sentinel"), "operator-only").unwrap();
    let alias = trusted.join("alias");
    std::os::unix::fs::symlink(&private, &alias).unwrap();
    let result = resolve_source(&alias, |target| target.starts_with(&trusted));
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("environment prerequisite")
    );
    fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&tree.0, &alias).unwrap();
    assert!(resolve_source(&alias, |target| target.starts_with(&trusted)).is_err());
    fs::remove_file(&alias).unwrap();
    let binary = trusted.join("binary");
    fs::write(&binary, "trusted runtime").unwrap();
    std::os::unix::fs::symlink(&binary, &alias).unwrap();
    assert_eq!(
        resolve_source(&alias, |target| target.starts_with(&trusted)).unwrap(),
        Some(binary)
    );
    fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(trusted.join("missing"), &alias).unwrap();
    assert!(resolve_source(&alias, |target| target.starts_with(&trusted)).is_err());
    assert_eq!(
        resolve_source(&trusted.join("optional-missing"), |_| false).unwrap(),
        None
    );
}

#[test]
fn project_roots_cannot_turn_into_blanket_host_or_runtime_mounts() {
    for root in [
        "/",
        "/usr",
        "/usr/bin/project",
        "/usr/local",
        "/etc/project",
        "/proc",
        "/home",
        "/root",
        "/tmp",
        "/opt",
        "/run/project",
        "/var/lib/docker",
        "/var/log/project",
        "/home/operator/.ssh",
        "/home/operator/.aws",
        "/home/operator/project/.git",
    ] {
        assert!(
            validate_workspace_root(Path::new(root)).is_err(),
            "accepted {root}"
        );
    }
    let tree = TestTree::new();
    assert!(validate_workspace_root(&tree.0).is_ok());
    assert!(validate_workspace_root(Path::new("/usr/local/src/project")).is_ok());
}

#[test]
fn visibility_requires_actual_runtime_paths_and_precise_build_data_versions() {
    for path in [
        "/usr/share/dotnet",
        "/usr/share/node-v24/bin/node",
        "/usr/local/src/node/bin/node",
        "/usr/share/cmake-secrets",
        "/usr/share/cmake-3../secrets",
        "/usr/share/automake-1.2-private",
    ] {
        assert!(
            !runtime_visible(Path::new(path)),
            "unexpectedly visible {path}"
        );
    }
    for path in [
        "/usr/local/lib/node_modules/npm/bin/npm-cli.js",
        "/usr/lib/dotnet",
        "/usr/share/perl/5.38/CPAN.pm",
        "/usr/share/cmake-3.28/Modules",
        "/usr/local/share/automake-1.17/Automake",
        "/usr/share/aclocal-1.16",
    ] {
        assert!(runtime_visible(Path::new(path)), "missing runtime {path}");
    }
}

#[test]
fn distro_os_configuration_redirects_are_explicit_and_narrow() {
    assert!(os_config_source_allowed(
        Path::new("/etc/nsswitch.conf"),
        Path::new("/etc/authselect/nsswitch.conf")
    ));
    assert!(!os_config_source_allowed(
        Path::new("/etc/nsswitch.conf"),
        Path::new("/etc/authselect/authselect.conf")
    ));
    assert!(!os_config_source_allowed(
        Path::new("/etc/nsswitch.conf"),
        Path::new("/etc/passwd")
    ));
    assert!(os_config_source_allowed(
        Path::new("/etc/localtime"),
        Path::new("/usr/share/zoneinfo/Etc/UTC")
    ));
    assert!(!os_config_source_allowed(
        Path::new("/etc/localtime"),
        Path::new("/usr/share/zoneinfo-private/Etc/UTC")
    ));
}

#[test]
fn distro_certificate_roots_are_mounted_only_at_public_locations() {
    assert!(public_ca_source_allowed(
        Path::new("/etc/ssl/certs"),
        Path::new("/etc/pki/tls/certs")
    ));
    assert!(public_ca_source_allowed(
        Path::new("/etc/pki/tls/certs"),
        Path::new("/etc/ssl/certs")
    ));
    assert!(public_ca_source_allowed(
        Path::new("/etc/pki/ca-trust/extracted/pem"),
        Path::new("/etc/pki/ca-trust/extracted/pem")
    ));
    assert!(!public_ca_source_allowed(
        Path::new("/etc/ssl/certs"),
        Path::new("/etc/pki/private")
    ));
    assert!(!public_ca_source_allowed(
        Path::new("/etc/pki/ca-trust/extracted/pem"),
        Path::new("/etc/pki/ca-trust")
    ));
}
