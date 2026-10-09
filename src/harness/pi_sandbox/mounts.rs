use std::path::{Path, PathBuf};

use super::components::ensure_narrow_host_directory;

mod system_runtime;
pub(super) use system_runtime::{mount_system_runtime, runtime_visible, validate_workspace_root};

pub(super) fn mount_toolchains(
    args: &mut Vec<String>,
    created: &mut std::collections::BTreeSet<String>,
) -> anyhow::Result<bool> {
    let home = std::env::var_os("HOME").and_then(|path| PathBuf::from(path).canonicalize().ok());
    let cargo = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|path| path.join(".cargo")))
        .and_then(|path| approved_home_directory(path, home.as_deref()));
    let rustup = std::env::var_os("RUSTUP_HOME")
        .map(PathBuf::from)
        .or_else(|| home.as_ref().map(|path| path.join(".rustup")))
        .and_then(|path| approved_home_directory(path, home.as_deref()));
    if let Some(cargo) = cargo {
        for (name, source) in [
            ("koolade-tools/cargo-bin", cargo.join("bin")),
            ("koolade-tools/cargo-home/registry", cargo.join("registry")),
            ("koolade-tools/cargo-home/git", cargo.join("git")),
        ] {
            if let Some(source) = approved_home_directory(source, home.as_deref()) {
                let destination = Path::new("/tmp").join(name);
                bind_readonly(args, created, &source, &destination)?;
            }
        }
    }
    let mounted_rustup = if let Some(rustup) = rustup {
        bind_readonly(
            args,
            created,
            &rustup,
            Path::new("/tmp/koolade-tools/rustup-home"),
        )?;
        true
    } else {
        false
    };
    make_dir(args, created, Path::new("/tmp/koolade-tools/cargo-home"));
    Ok(mounted_rustup)
}

fn approved_home_directory(path: PathBuf, home: Option<&Path>) -> Option<PathBuf> {
    let home = home?;
    let path = path.canonicalize().ok()?;
    if path == home || !path.starts_with(home) || !path.is_dir() {
        return None;
    }
    ensure_narrow_host_directory(&path, "host toolchain directory")
        .ok()
        .map(|()| path)
}

pub(super) fn bind_readonly(
    args: &mut Vec<String>,
    created: &mut std::collections::BTreeSet<String>,
    source: &Path,
    destination: &Path,
) -> anyhow::Result<()> {
    ensure_parents(args, created, destination);
    if destination != Path::new("/") {
        make_dir(args, created, destination);
    }
    args.extend([
        "--ro-bind".into(),
        source.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    ]);
    Ok(())
}

pub(super) fn bind_readonly_file(
    args: &mut Vec<String>,
    created: &mut std::collections::BTreeSet<String>,
    source: &Path,
    destination: &Path,
) {
    ensure_parents(args, created, destination);
    args.extend([
        "--ro-bind".into(),
        source.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    ]);
}

pub(super) fn bind_readwrite(
    args: &mut Vec<String>,
    created: &mut std::collections::BTreeSet<String>,
    source: &Path,
    destination: &Path,
) -> anyhow::Result<()> {
    ensure_parents(args, created, destination);
    make_dir(args, created, destination);
    args.extend([
        "--bind".into(),
        source.to_string_lossy().into_owned(),
        destination.to_string_lossy().into_owned(),
    ]);
    Ok(())
}

pub(super) fn mount_tmpfs(
    args: &mut Vec<String>,
    created: &mut std::collections::BTreeSet<String>,
    path: &Path,
    size: u64,
) {
    ensure_parents(args, created, path);
    make_dir(args, created, path);
    args.extend([
        "--size".into(),
        size.to_string(),
        "--tmpfs".into(),
        path.to_string_lossy().into_owned(),
    ]);
}

pub(super) fn make_dir(
    args: &mut Vec<String>,
    created: &mut std::collections::BTreeSet<String>,
    path: &Path,
) {
    let value = path.to_string_lossy().into_owned();
    if created.insert(value.clone()) {
        args.extend(["--dir".into(), value]);
    }
}

fn ensure_parents(
    args: &mut Vec<String>,
    created: &mut std::collections::BTreeSet<String>,
    path: &Path,
) {
    let mut current = PathBuf::from("/");
    for component in path.parent().into_iter().flat_map(Path::components) {
        if let std::path::Component::Normal(part) = component {
            current.push(part);
            make_dir(args, created, &current);
        }
    }
}

pub(super) fn push_env(args: &mut Vec<String>, name: &str, value: &str) {
    args.extend(["--setenv".into(), name.into(), value.into()]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "koolade-toolchain-mounts-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

    #[test]
    fn toolchain_mounts_reject_sensitive_paths_and_symlink_targets() {
        let root = root();
        let home = root.join("home");
        let sensitive = home.join(".ssh");
        fs::create_dir_all(&sensitive).unwrap();
        fs::create_dir_all(home.join(".cargo")).unwrap();
        fs::create_dir_all(home.join(".rustup")).unwrap();
        assert!(approved_home_directory(sensitive.clone(), Some(&home)).is_none());

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let rustup = home.join(".rustup");
            fs::remove_dir(&rustup).unwrap();
            symlink(&sensitive, &rustup).unwrap();
            assert!(approved_home_directory(rustup, Some(&home)).is_none());

            let cargo_bin = home.join(".cargo/bin");
            symlink(&sensitive, &cargo_bin).unwrap();
            assert!(approved_home_directory(cargo_bin, Some(&home)).is_none());
        }

        fs::remove_dir_all(root).unwrap();
    }
}
