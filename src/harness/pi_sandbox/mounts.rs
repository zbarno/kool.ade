use std::path::{Path, PathBuf};

/**
 * Construct the same explicit, read-only system-runtime view for both planning
 * and implementation. There is deliberately no bind of the host's root (/),
 * /opt, /var, or /etc directory: they may contain private operator data.
 *
 * Keep the allowlist narrow and add tool-specific mounts separately instead of
 * expanding this list to make a single project's verification succeed.
 */
pub(super) fn mount_system_runtime(
    args: &mut Vec<String>,
    created: &mut std::collections::BTreeSet<String>,
) -> anyhow::Result<()> {
    // Dynamic loader, common shells and system tools. Bind the named paths
    // rather than using the host root as a read-only fallback.
    for location in ["/usr", "/bin", "/sbin", "/lib", "/lib64"] {
        let path = Path::new(location);
        if path.exists() {
            bind_readonly(args, created, path, path)?;
        }
    }
    // This is a build/source staging area, not part of the trusted runtime.
    if Path::new("/usr/local/src").is_dir() {
        mount_tmpfs(args, created, Path::new("/usr/local/src"), 16_777_216);
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
        if path.is_file() {
            bind_readonly_file(args, created, path, path);
        }
    }
    // On Debian/Ubuntu, /usr/bin/cc and /usr/bin/c++ are symlinks into
    // /etc/alternatives. Do not expose the alternatives directory wholesale:
    // expose only compiler aliases resolving to files in the trusted runtime.
    for tool in ["cc", "c++", "cpp"] {
        let alias = Path::new("/etc/alternatives").join(tool);
        if let Ok(target) = alias.canonicalize()
            && target.is_file()
            && target.starts_with("/usr")
        {
            bind_readonly_file(args, created, &target, &alias);
        }
    }
    // Public CA roots are safe to expose; never bind the parent /etc/ssl.
    let certs = Path::new("/etc/ssl/certs");
    if certs.is_dir() {
        bind_readonly(args, created, certs, certs)?;
    }
    Ok(())
}

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
    (path != home && path.starts_with(home) && path.is_dir()).then_some(path)
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
