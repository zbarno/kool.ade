use std::{
    collections::BTreeSet,
    env,
    path::{Path, PathBuf},
};

use super::path_safety::ensure_narrow_host_directory;
use crate::harness::pi_sandbox::mounts::{bind_readonly, bind_readonly_file, make_dir};

const VISIBLE_RUNTIME_ROOTS: [&str; 6] = ["/usr", "/bin", "/sbin", "/lib", "/lib64", "/usr/local"];
const SANDBOX_NODE_ROOT: &str = "/tmp/koolade-tools/node";

pub(in crate::harness::pi_sandbox) fn host_node_root() -> anyhow::Result<Option<PathBuf>> {
    let Some(path) = env::var_os("PATH") else {
        return Ok(None);
    };
    for directory in env::split_paths(&path).filter(|directory| directory.is_absolute()) {
        let executable = directory.join("node");
        let Ok(executable) = executable.canonicalize() else {
            continue;
        };
        if VISIBLE_RUNTIME_ROOTS
            .iter()
            .any(|root| executable.starts_with(root))
        {
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

pub(super) fn mount_node_runtime(
    args: &mut Vec<String>,
    created: &mut BTreeSet<String>,
    empty_file: &Path,
) -> anyhow::Result<Option<PathBuf>> {
    let Some(root) = host_node_root()? else {
        return Ok(None);
    };
    let node = root.join("bin/node").canonicalize()?;
    let npm = root
        .join("lib/node_modules/npm/bin/npm-cli.js")
        .canonicalize()?;
    anyhow::ensure!(
        node.starts_with(&root) && npm.starts_with(&root),
        "Node.js installation contains a symlink escape"
    );
    let npm_bin = root.join("bin/npm").canonicalize()?;
    anyhow::ensure!(
        npm_bin.starts_with(&root),
        "Node.js npm launcher resolves outside its installation"
    );
    let destination = PathBuf::from(SANDBOX_NODE_ROOT);
    make_dir(args, created, &destination);
    bind_readonly(args, created, &root, &destination)?;
    let global_config = root.join("etc/npmrc");
    if global_config.is_file() {
        bind_readonly_file(args, created, empty_file, &destination.join("etc/npmrc"));
    }
    Ok(Some(destination))
}

pub(super) fn add_node_path(path: &mut String, node_root: Option<&Path>) {
    if let Some(root) = node_root {
        *path = format!("{}/bin:{path}", root.display());
    }
}
