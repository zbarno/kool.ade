use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use super::{mount_dotnet_root, prepare_nuget_packages};

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "koolade-components-{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
}

#[test]
fn user_dotnet_install_is_mounted_without_exposing_neighboring_home_files() {
    let root = temp_root("dotnet");
    let install = root.join(".dotnet");
    fs::create_dir_all(install.join("sdk")).unwrap();
    fs::create_dir_all(install.join("shared")).unwrap();
    fs::write(install.join("dotnet"), "dotnet host").unwrap();
    fs::write(root.join(".ssh-key"), "credential sentinel").unwrap();

    let mut args = Vec::new();
    let mut created = BTreeSet::from(["/".to_owned()]);
    let mounted = mount_dotnet_root(&mut args, &mut created, &install).unwrap();

    assert_eq!(mounted, Path::new("/tmp/koolade-tools/dotnet"));
    assert!(args.windows(3).any(|mount| {
        mount[0] == "--ro-bind"
            && mount[1]
                == install
                    .join("dotnet")
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
            && mount[2] == "/tmp/koolade-tools/dotnet/dotnet"
    }));
    for name in ["sdk", "shared"] {
        assert!(args.windows(3).any(|mount| {
            mount[0] == "--ro-bind"
                && mount[1] == install.join(name).canonicalize().unwrap().to_string_lossy()
                && mount[2] == format!("/tmp/koolade-tools/dotnet/{name}")
        }));
    }
    assert!(
        !args
            .iter()
            .any(|arg| arg == root.to_string_lossy().as_ref())
    );

    let host_home = PathBuf::from(std::env::var_os("HOME").unwrap());
    assert!(mount_dotnet_root(&mut args, &mut created, &host_home).is_err());
    assert!(mount_dotnet_root(&mut args, &mut created, Path::new("/tmp")).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn dotnet_symlinks_cannot_escape_the_installation_directory() {
    use std::os::unix::fs::symlink;

    let root = temp_root("dotnet-symlinks");
    let install = root.join(".dotnet");
    let external = root.join("external");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(external.join("sdk")).unwrap();

    fs::write(external.join("dotnet"), "external dotnet").unwrap();
    symlink(external.join("dotnet"), install.join("dotnet")).unwrap();
    let mut args = Vec::new();
    let mut created = BTreeSet::from(["/".to_owned()]);
    assert!(mount_dotnet_root(&mut args, &mut created, &install).is_err());

    fs::remove_file(install.join("dotnet")).unwrap();
    fs::write(install.join("dotnet"), "local dotnet").unwrap();
    symlink(external.join("sdk"), install.join("sdk")).unwrap();
    assert!(mount_dotnet_root(&mut args, &mut created, &install).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn nuget_cache_defaults_to_the_host_packages_directory() {
    let root = temp_root("nuget");
    let home = root.join("operator-home");
    let current = root.join("checkout");
    fs::create_dir_all(&current).unwrap();

    let packages = prepare_nuget_packages(None, Some(home.clone()), &current).unwrap();

    assert_eq!(
        packages,
        home.join(".nuget/packages").canonicalize().unwrap()
    );
    assert!(packages.is_dir());
    assert!(
        prepare_nuget_packages(Some(home.clone()), Some(home.clone()), &current).is_err(),
        "the host home itself must never be mounted as a package cache"
    );
    let credentials_cache = home.join(".aws/packages");
    assert!(
        prepare_nuget_packages(Some(credentials_cache), Some(home.clone()), &current).is_err(),
        "a path inside a credentials directory must never be mounted"
    );
    assert!(
        prepare_nuget_packages(Some(PathBuf::from("/tmp")), Some(home.clone()), &current).is_err(),
        "a broad temporary directory must never be mounted"
    );
    for protected in [
        "/etc/ssh/private/packages",
        "/etc/ssl/private/packages",
        "/run/secrets/packages",
        "/var/lib/secrets/packages",
        "/var/lib/sss/packages",
        "/var/lib/sssd/packages",
        "/var/lib/NetworkManager/packages",
        "/var/lib/kubelet/packages",
    ] {
        assert!(
            prepare_nuget_packages(Some(PathBuf::from(protected)), Some(home.clone()), &current)
                .is_err(),
            "protected system path {protected} must never be mounted"
        );
    }
    let temporary_cache = root.join("temporary-cache/nuget-packages");
    assert!(
        prepare_nuget_packages(Some(temporary_cache), Some(home.clone()), &current).is_err(),
        "an ephemeral cache outside the host home must never be mounted"
    );
    let cache = home.join(".nuget/packages");
    fs::create_dir_all(cache.join("NuGet")).unwrap();
    fs::write(cache.join("NuGet/NuGet.Config"), "credential sentinel").unwrap();
    assert!(
        prepare_nuget_packages(Some(cache), Some(home.clone()), &current).is_err(),
        "NuGet configuration and credentials must remain hidden"
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn nuget_cache_symlink_cannot_redirect_into_credentials() {
    use std::os::unix::fs::symlink;

    let root = temp_root("nuget-symlink");
    let home = root.join("operator-home");
    let current = root.join("checkout");
    let credentials = home.join(".aws/packages");
    let cache = home.join(".nuget/packages");
    fs::create_dir_all(&current).unwrap();
    fs::create_dir_all(&credentials).unwrap();
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    symlink(&credentials, &cache).unwrap();

    assert!(prepare_nuget_packages(Some(cache), Some(home), &current).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn relative_nuget_override_is_resolved_on_the_host() {
    let root = temp_root("relative-nuget");
    let home = root.join("operator-home");
    let current = home.join("checkout");
    fs::create_dir_all(&current).unwrap();

    let packages = prepare_nuget_packages(
        Some(PathBuf::from(".cache/nuget-packages")),
        Some(home),
        &current,
    )
    .unwrap();

    assert_eq!(
        packages,
        current
            .join(".cache/nuget-packages")
            .canonicalize()
            .unwrap()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn dedicated_nuget_cache_outside_home_is_supported() {
    let cache_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("koolade-nuget-cache-{}", uuid::Uuid::new_v4()));
    let packages_path = cache_root.join("nuget-packages");
    let root = temp_root("external-nuget");
    let home = root.join("operator-home");
    let current = root.join("checkout");
    fs::create_dir_all(&current).unwrap();

    let packages =
        prepare_nuget_packages(Some(packages_path.clone()), Some(home), &current).unwrap();

    assert_eq!(packages, packages_path.canonicalize().unwrap());
    fs::remove_dir_all(cache_root).unwrap();
    fs::remove_dir_all(root).unwrap();
}
