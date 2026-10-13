//! Narrow discovery for host tools used by controller-side dependency brokers.
use std::{
    env,
    path::{Path, PathBuf},
};

mod path_safety;
use path_safety::ensure_narrow_host_directory;

pub(crate) fn host_node_root_for_resource_broker() -> anyhow::Result<Option<PathBuf>> {
    let Some(path) = env::var_os("PATH") else {
        return Ok(None);
    };
    for directory in env::split_paths(&path).filter(|directory| directory.is_absolute()) {
        let Ok(executable) = directory.join("node").canonicalize() else {
            continue;
        };
        if system_runtime_visible(&executable) {
            return Ok(None);
        }
        let Some(root) = executable.parent().and_then(Path::parent) else {
            continue;
        };
        let Ok(root) = root.canonicalize() else {
            continue;
        };
        let node = root.join("bin/node");
        let npm = root.join("lib/node_modules/npm/bin/npm-cli.js");
        if node.canonicalize().ok().as_deref() != Some(executable.as_path()) || !npm.is_file() {
            continue;
        }
        ensure_narrow_host_directory(&root, "Node.js installation")?;
        return Ok(Some(root));
    }
    Ok(None)
}

pub(crate) fn host_dotnet_executable() -> Option<PathBuf> {
    for variable in ["DOTNET_ROOT", "DOTNET_ROOT_X64", "DOTNET_ROOT_X86"] {
        if let Some(executable) =
            env::var_os(variable).and_then(|value| dotnet_executable(Path::new(&value)))
        {
            return Some(executable);
        }
    }
    env::split_paths(&env::var_os("PATH")?)
        .filter(|directory| directory.is_absolute())
        .find_map(|directory| dotnet_executable(&directory.join("dotnet")))
}

fn dotnet_executable(path: &Path) -> Option<PathBuf> {
    let executable = if path.is_dir() {
        path.join("dotnet")
    } else {
        path.to_path_buf()
    };
    let executable = executable.canonicalize().ok()?;
    if !executable.is_file() {
        return None;
    }
    let root = executable.parent()?.canonicalize().ok()?;
    ensure_narrow_host_directory(&root, ".NET installation").ok()?;
    Some(executable)
}

fn system_runtime_visible(path: &Path) -> bool {
    ["/usr", "/bin", "/sbin", "/lib", "/lib64"]
        .iter()
        .any(|root| path.starts_with(root))
}
