use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
};

use super::super::mounts::{bind_readonly, bind_readonly_file, make_dir};
use super::{SANDBOX_DOTNET_ROOT, path_safety::ensure_narrow_host_directory};

const VISIBLE_RUNTIME_ROOTS: [&str; 6] = ["/usr", "/bin", "/sbin", "/lib", "/lib64", "/usr/local"];
const DOTNET_LAYOUT_DIRS: [&str; 9] = [
    "host",
    "sdk",
    "sdk-manifests",
    "shared",
    "packs",
    "templates",
    "metadata",
    "tools",
    "workloadmanifests",
];

pub(super) fn host_dotnet_root() -> Option<PathBuf> {
    for variable in ["DOTNET_ROOT", "DOTNET_ROOT_X64", "DOTNET_ROOT_X86"] {
        if let Some(root) = env::var_os(variable).and_then(|value| dotnet_root(Path::new(&value))) {
            return Some(root);
        }
    }

    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .filter(|directory| directory.is_absolute())
        .find_map(|directory| dotnet_root(&directory.join("dotnet")))
}

fn dotnet_root(path: &Path) -> Option<PathBuf> {
    let executable = if path.is_dir() {
        path.join("dotnet")
    } else {
        path.to_path_buf()
    };
    let executable = executable.canonicalize().ok()?;
    if !executable.is_file() {
        return None;
    }
    executable.parent()?.canonicalize().ok()
}

pub(super) fn mount_dotnet_root(
    args: &mut Vec<String>,
    created: &mut BTreeSet<String>,
    source: &Path,
) -> anyhow::Result<PathBuf> {
    let source = source.canonicalize()?;
    anyhow::ensure!(
        source.is_dir(),
        "DOTNET_ROOT must be an installation directory"
    );
    if VISIBLE_RUNTIME_ROOTS
        .iter()
        .any(|root| source.starts_with(root))
    {
        return Ok(source);
    }
    ensure_narrow_host_directory(&source, "DOTNET_ROOT")?;

    let executable = source.join("dotnet").canonicalize()?;
    anyhow::ensure!(
        executable.is_file() && executable.starts_with(&source),
        "DOTNET_ROOT must contain an executable inside its installation directory"
    );

    let target = PathBuf::from(SANDBOX_DOTNET_ROOT);
    make_dir(args, created, &target);
    bind_readonly_file(args, created, &executable, &target.join("dotnet"));
    for name in DOTNET_LAYOUT_DIRS {
        let child = source.join(name);
        if child.is_dir() {
            let child = child.canonicalize()?;
            anyhow::ensure!(
                child.starts_with(&source),
                "DOTNET_ROOT component {} resolves outside its installation directory",
                child.display()
            );
            bind_readonly(args, created, &child, &target.join(name))?;
        }
    }
    Ok(target)
}
