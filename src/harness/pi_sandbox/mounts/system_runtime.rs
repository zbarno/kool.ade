//! Trusted OS runtime directories, never their data/configuration parents.
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use super::{bind_readonly, bind_readonly_file, ensure_parents, make_dir};

const RUNTIME_DIRS: &[&str] = &[
    "/usr/bin",
    "/usr/sbin",
    "/usr/lib",
    "/usr/lib64",
    "/usr/libexec",
    "/usr/include",
    "/usr/local/bin",
    "/usr/local/sbin",
    "/usr/local/lib",
    "/usr/local/lib64",
    "/usr/local/libexec",
    "/usr/local/include",
    "/usr/share/locale",
    "/usr/share/zoneinfo",
    "/usr/share/nodejs",
    "/usr/share/npm",
    "/usr/share/ca-certificates",
    "/usr/share/git-core",
    "/usr/share/perl",
    "/usr/share/perl5",
    "/usr/share/cmake",
    "/usr/share/autoconf",
    "/usr/share/aclocal",
    "/usr/share/libtool",
    "/usr/local/share/perl",
    "/usr/local/share/perl5",
    "/usr/local/share/cmake",
    "/usr/local/share/autoconf",
    "/usr/local/share/aclocal",
    "/usr/local/share/libtool",
];
const RUNTIME_ALIASES: &[&str] = &["/bin", "/sbin", "/lib", "/lib64"];

pub(in crate::harness::pi_sandbox) fn runtime_visible(path: &Path) -> bool {
    RUNTIME_DIRS
        .iter()
        .chain(RUNTIME_ALIASES)
        .any(|root| path.starts_with(root))
        || versioned_build_data(path)
}

fn versioned_build_data(path: &Path) -> bool {
    ["/usr/share", "/usr/local/share"].iter().any(|parent| {
        let Some(name) = path
            .strip_prefix(parent)
            .ok()
            .and_then(|relative| relative.components().next())
            .and_then(|part| part.as_os_str().to_str())
        else {
            return false;
        };
        ["cmake-", "automake-", "aclocal-"].iter().any(|prefix| {
            name.strip_prefix(prefix).is_some_and(|version| {
                !version.is_empty()
                    && version.split('.').all(|part| {
                        !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())
                    })
            })
        })
    })
}

pub(in crate::harness::pi_sandbox) fn validate_workspace_root(root: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        root.is_absolute()
            && !runtime_visible(root)
            && root != Path::new("/")
            && ![
                "/home",
                "/root",
                "/tmp",
                "/var",
                "/opt",
                "/mnt",
                "/media",
                "/run",
                "/srv",
                "/workspace",
                "/workspaces"
            ]
            .iter()
            .any(|path| root == Path::new(path))
            && ![
                "/etc", "/proc", "/sys", "/dev", "/boot", "/run", "/var/lib", "/var/log"
            ]
            .iter()
            .any(|path| root.starts_with(path))
            && !root
                .components()
                .any(|part| part.as_os_str().to_str().is_some_and(|name| {
                    name == ".git"
                        || super::super::components::SENSITIVE_HOST_PATH_COMPONENTS.contains(&name)
                }))
            && !RUNTIME_DIRS
                .iter()
                .chain(RUNTIME_ALIASES)
                .any(|path| { root.starts_with(path) || Path::new(path).starts_with(root) })
            && !std::env::var_os("HOME")
                .and_then(|home| PathBuf::from(home).canonicalize().ok())
                .is_some_and(|home| home.starts_with(root)),
        "Sandbox environment prerequisite: select a dedicated project directory outside system runtime and protected host directories"
    );
    Ok(())
}

pub(in crate::harness::pi_sandbox) fn mount_system_runtime(
    args: &mut Vec<String>,
    created: &mut BTreeSet<String>,
) -> anyhow::Result<()> {
    let mut locations: Vec<PathBuf> = RUNTIME_DIRS
        .iter()
        .chain(RUNTIME_ALIASES)
        .map(PathBuf::from)
        .collect();
    for parent in ["/usr/share", "/usr/local/share"] {
        match fs::read_dir(parent) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry?.path();
                    if versioned_build_data(&path) {
                        locations.push(path);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    locations.sort();
    for path in &locations {
        let Some(source) = resolve_source(path, runtime_visible)? else {
            continue;
        };
        anyhow::ensure!(
            source.is_dir(),
            "Sandbox environment prerequisite: runtime mount must be a directory"
        );
        bind_readonly(args, created, &source, path)?;
    }
    make_dir(args, created, Path::new("/etc"));
    for location in [
        "/etc/ld.so.cache",
        "/etc/passwd",
        "/etc/group",
        "/etc/nsswitch.conf",
        "/etc/localtime",
    ] {
        let path = Path::new(location);
        let Some(source) = resolve_source(path, |target| {
            target == path
                || (location == "/etc/localtime" && target.starts_with("/usr/share/zoneinfo"))
        })?
        else {
            continue;
        };
        anyhow::ensure!(
            source.is_file(),
            "Sandbox environment prerequisite: OS configuration must be a regular file"
        );
        bind_readonly_file(args, created, &source, path);
    }
    // Preserve canonical compiler and build-tool executable paths without
    // exposing the alternatives directory or unrelated alternatives.
    for tool in ["cc", "c++", "cpp", "automake", "aclocal"] {
        let alias = Path::new("/etc/alternatives").join(tool);
        let Some(target) = resolve_source(&alias, runtime_visible)? else {
            continue;
        };
        anyhow::ensure!(
            target.is_file(),
            "Sandbox environment prerequisite: tool alias must resolve to a runtime file"
        );
        ensure_parents(args, created, &alias);
        args.extend([
            "--symlink".into(),
            target.to_string_lossy().into_owned(),
            alias.to_string_lossy().into_owned(),
        ]);
    }
    let certs = Path::new("/etc/ssl/certs");
    if let Some(source) = resolve_source(certs, |target| {
        target == certs || target.starts_with("/usr/share/ca-certificates")
    })? {
        anyhow::ensure!(
            source.is_dir(),
            "Sandbox environment prerequisite: CA certificate mount must be a directory"
        );
        bind_readonly(args, created, &source, certs)?;
    }
    Ok(())
}

fn resolve_source(
    path: &Path,
    permitted: impl FnOnce(&Path) -> bool,
) -> anyhow::Result<Option<PathBuf>> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    let source = path.canonicalize().map_err(|error| {
        anyhow::anyhow!(
            "Sandbox environment prerequisite: cannot resolve runtime mount {}: {error}",
            path.display()
        )
    })?;
    anyhow::ensure!(
        permitted(&source),
        "Sandbox environment prerequisite: runtime mount {} resolves outside its allowlist",
        path.display()
    );
    Ok(Some(source))
}

#[cfg(test)]
mod tests;
