use std::{
    env,
    path::{Component, Path, PathBuf},
};

const BROAD_HOST_DIRECTORIES: [&str; 14] = [
    "/home",
    "/root",
    "/tmp",
    "/mnt",
    "/media",
    "/run",
    "/var",
    "/srv",
    "/etc",
    "/opt",
    "/workspace",
    "/workspaces",
    "/boot",
    "/sys",
];
const MASKED_HOME_ROOTS: [&str; 2] = ["/home", "/root"];
pub(in crate::harness::pi_sandbox) const SENSITIVE_HOST_PATH_COMPONENTS: [&str; 14] = [
    ".aws",
    ".ssh",
    ".config",
    ".kube",
    ".azure",
    ".docker",
    ".gnupg",
    ".pki",
    "keyrings",
    "credentials",
    "secret",
    "secrets",
    ".secret",
    ".secrets",
];
const PROTECTED_HOST_SUBTREES: [&str; 16] = [
    "/etc",
    "/run",
    "/proc",
    "/sys",
    "/dev",
    "/boot",
    "/var/lib/sss",
    "/var/lib/sssd",
    "/var/lib/NetworkManager",
    "/var/lib/kubelet",
    "/var/lib/private",
    "/var/lib/systemd/credential",
    "/var/lib/systemd/credentials",
    "/var/lib/docker",
    "/var/lib/containerd",
    "/var/lib/containers",
];

pub(super) fn ensure_narrow_host_directory(path: &Path, label: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        path != Path::new("/"),
        "{label} must identify a specific installation or cache directory"
    );
    anyhow::ensure!(
        !BROAD_HOST_DIRECTORIES
            .iter()
            .any(|root| path == Path::new(root)),
        "{label} cannot point to a broad host directory"
    );
    anyhow::ensure!(
        !PROTECTED_HOST_SUBTREES
            .iter()
            .any(|root| path.starts_with(root)),
        "{label} cannot expose protected system data"
    );
    let home = env::var_os("HOME")
        .and_then(|value| canonical_path_with_missing_tail(&PathBuf::from(value)).ok());
    if MASKED_HOME_ROOTS.iter().any(|root| path.starts_with(root)) {
        let home = home.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Cannot validate a home-directory component without HOME")
        })?;
        anyhow::ensure!(
            path.starts_with(home),
            "{label} cannot expose another home directory"
        );
    }
    if let Some(home) = home {
        anyhow::ensure!(
            path != home && !home.starts_with(path),
            "{label} cannot expose the host home directory or one of its parents"
        );
    }
    Ok(())
}

pub(super) fn canonical_path_with_missing_tail(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    let absolute = normalize_absolute_path(&absolute)?;
    let mut existing = absolute.clone();
    let mut missing = Vec::new();
    while !existing.exists() {
        let name = existing
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Cannot resolve path {}", absolute.display()))?;
        missing.push(name.to_os_string());
        anyhow::ensure!(existing.pop(), "Cannot resolve path {}", absolute.display());
    }
    let mut canonical = existing.canonicalize()?;
    for name in missing.into_iter().rev() {
        canonical.push(name);
    }
    Ok(canonical)
}

fn normalize_absolute_path(path: &Path) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(path.is_absolute(), "Path must be absolute");
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(name) => normalized.push(name),
            Component::Prefix(_) => anyhow::bail!("Unsupported path prefix"),
        }
    }
    Ok(normalized)
}
