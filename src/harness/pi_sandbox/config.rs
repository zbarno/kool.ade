use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use super::mounts::{
    bind_readonly, bind_readonly_file, bind_readwrite, make_dir, mount_tmpfs, mount_toolchains,
    push_env,
};

pub(super) fn locate_bwrap(root: &Path) -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("PACKET_BWRAP_BIN") {
        let path = PathBuf::from(path).canonicalize()?;
        anyhow::ensure!(
            is_executable(&path),
            "PACKET_BWRAP_BIN must name an executable bubblewrap binary"
        );
        anyhow::ensure!(
            !inside_workspace(&path, root),
            "PACKET_BWRAP_BIN must not point inside a repository or task worktree"
        );
        return Ok(path);
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join("bwrap"))
        .filter(|candidate| is_executable(candidate))
        .filter_map(|candidate| candidate.canonicalize().ok())
        .find(|candidate| !inside_workspace(candidate, root))
        .ok_or_else(|| anyhow::anyhow!("Implementation is paused because bubblewrap (bwrap) is unavailable; install it before resuming"))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    false
}

pub(super) fn arguments(
    root: &Path,
    empty_file: &Path,
    pi_executable: Option<&Path>,
) -> anyhow::Result<(Vec<String>, PathBuf)> {
    let mut args = vec![
        "--die-with-parent".into(),
        "--unshare-user".into(),
        "--unshare-pid".into(),
        "--unshare-net".into(),
        "--unshare-ipc".into(),
        "--unshare-uts".into(),
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
    ];
    let mut created = BTreeSet::from(["/".to_owned()]);
    for path in [
        "/home",
        "/root",
        "/mnt",
        "/media",
        "/run",
        "/tmp",
        "/var",
        "/srv",
        "/workspace",
        "/workspaces",
        "/boot",
        "/sys",
    ] {
        if Path::new(path).is_dir() {
            mount_tmpfs(
                &mut args,
                &mut created,
                Path::new(path),
                if path == "/tmp" {
                    1_073_741_824
                } else {
                    67_108_864
                },
            );
        }
    }
    for path in [
        "/etc/ssh",
        "/etc/ssl/private",
        "/etc/letsencrypt",
        "/etc/docker",
        "/etc/containers",
        "/etc/NetworkManager/system-connections",
        "/etc/sudoers.d",
    ] {
        if Path::new(path).is_dir() {
            mount_tmpfs(&mut args, &mut created, Path::new(path), 16_777_216);
        }
    }
    for path in [
        "/etc/krb5.keytab",
        "/etc/shadow",
        "/etc/gshadow",
        "/etc/sudoers",
    ] {
        if Path::new(path).is_file() {
            bind_readonly_file(&mut args, &mut created, empty_file, Path::new(path));
        }
    }
    make_dir(&mut args, &mut created, Path::new("/proc"));
    args.extend(["--proc".into(), "/proc".into()]);
    make_dir(&mut args, &mut created, Path::new("/dev"));
    args.extend(["--dev".into(), "/dev".into()]);
    let has_rustup = mount_toolchains(&mut args, &mut created)?;
    bind_readwrite(&mut args, &mut created, root, root)?;
    let common_dir = mount_git_metadata(&mut args, &mut created, root, empty_file)?;
    if let Some(executable) = pi_executable {
        super::planning::mount_pi_install(&mut args, &mut created, executable)?;
    }
    make_dir(&mut args, &mut created, Path::new("/tmp/packet-home"));
    args.extend(["--chdir".into(), root.to_string_lossy().into_owned()]);
    args.push("--clearenv".into());
    push_env(&mut args, "HOME", "/tmp/packet-home");
    push_env(&mut args, "TMPDIR", "/tmp");
    push_env(
        &mut args,
        "PATH",
        "/tmp/packet-tools/cargo-bin:/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin",
    );
    push_env(&mut args, "PACKET_WORKTREE", &root.to_string_lossy());
    push_env(&mut args, "GIT_CONFIG_NOSYSTEM", "1");
    push_env(&mut args, "GIT_CONFIG_GLOBAL", "/dev/null");
    push_env(&mut args, "GIT_CONFIG_SYSTEM", "/dev/null");
    push_env(&mut args, "GIT_OPTIONAL_LOCKS", "0");
    push_env(&mut args, "GIT_TERMINAL_PROMPT", "0");
    push_env(&mut args, "GIT_CONFIG_COUNT", "2");
    push_env(&mut args, "GIT_CONFIG_KEY_0", "credential.helper");
    push_env(&mut args, "GIT_CONFIG_VALUE_0", "");
    push_env(&mut args, "GIT_CONFIG_KEY_1", "core.hooksPath");
    push_env(&mut args, "GIT_CONFIG_VALUE_1", "/dev/null");
    push_env(&mut args, "CARGO_HOME", "/tmp/packet-tools/cargo-home");
    if has_rustup {
        push_env(&mut args, "RUSTUP_HOME", "/tmp/packet-tools/rustup-home");
    }
    Ok((args, common_dir))
}

fn mount_git_metadata(
    args: &mut Vec<String>,
    created: &mut BTreeSet<String>,
    root: &Path,
    empty_file: &Path,
) -> anyhow::Result<PathBuf> {
    let git_entry = root.join(".git");
    let metadata = fs::symlink_metadata(&git_entry).map_err(|_| {
        anyhow::anyhow!("Implementation must run from Packet's registered Git worktree")
    })?;
    anyhow::ensure!(
        metadata.is_file(),
        "Implementation must run from Packet's registered Git worktree"
    );
    let admin = git_path(root, "--git-dir")?;
    let common = git_path(root, "--git-common-dir")?;
    validate_packet_worktree(root, &admin, &common)?;
    bind_readonly_file(args, created, &git_entry, &git_entry);
    bind_readonly(args, created, &common, &common)?;
    if admin != common && !admin.starts_with(&common) {
        bind_readonly(args, created, &admin, &admin)?;
    }
    let worktrees = common.join("worktrees");
    if fs::symlink_metadata(&worktrees).is_ok_and(|metadata| metadata.is_dir()) {
        mount_tmpfs(args, created, &worktrees, 67_108_864);
        bind_readonly(args, created, &admin, &admin)?;
    }
    let diagnostics = common.join("packet-harness");
    match fs::symlink_metadata(&diagnostics) {
        Ok(metadata) => anyhow::ensure!(
            metadata.is_dir(),
            "Harness diagnostics directory must not be a symlink or regular file"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&diagnostics)?;
        }
        Err(error) => return Err(error.into()),
    }
    mount_tmpfs(args, created, &diagnostics, 67_108_864);
    mask_git_config(args, created, empty_file, &common.join("config"))?;
    if admin != common {
        mask_git_config(args, created, empty_file, &admin.join("config.worktree"))?;
    }
    Ok(common)
}

fn validate_packet_worktree(root: &Path, admin: &Path, common: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        admin != common && admin.starts_with(common.join("worktrees")),
        "Implementation worktree is not registered under its repository Git metadata"
    );
    anyhow::ensure!(
        common.file_name().is_some_and(|name| name == ".git"),
        "Implementation worktree has an unexpected Git metadata location"
    );
    let repository = common
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Git metadata has no repository parent"))?;
    let workspace_root = repository
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Repository has no workspace parent"))?;
    let project_directory = root
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Worktree has no project directory"))?;
    let worktree_registry = project_directory
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Worktree has no Packet registry"))?;
    let expected_slug = crate::persistence::project_slug(repository);
    anyhow::ensure!(
        worktree_registry
            .file_name()
            .is_some_and(|name| name == ".packet-worktrees")
            && worktree_registry.parent() == Some(workspace_root)
            && project_directory
                .file_name()
                .is_some_and(|name| name == std::ffi::OsStr::new(&expected_slug)),
        "Implementation worktree is outside Packet's isolated worktree directory"
    );
    Ok(())
}

fn git_path(root: &Path, option: &str) -> anyhow::Result<PathBuf> {
    let output = Command::new(locate_git(root)?)
        .args(["rev-parse", "--path-format=absolute", option])
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Cannot safely locate this worktree's Git metadata"
    );
    let path = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let path = path.canonicalize()?;
    anyhow::ensure!(path.is_dir(), "Git metadata path is not a directory");
    Ok(path)
}

pub(super) fn locate_git(root: &Path) -> anyhow::Result<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path).filter(|directory| directory.is_absolute()) {
        let candidate = directory.join("git");
        if !is_executable(&candidate) {
            continue;
        }
        let Ok(candidate) = candidate.canonicalize() else {
            continue;
        };
        if inside_workspace(&candidate, root) {
            continue;
        }
        return Ok(candidate);
    }
    anyhow::bail!("Cannot locate a trusted Git executable outside the task workspace")
}

fn inside_workspace(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
        || root
            .ancestors()
            .find(|ancestor| {
                ancestor
                    .file_name()
                    .is_some_and(|name| name == ".packet-worktrees")
            })
            .and_then(Path::parent)
            .is_some_and(|workspace| path.starts_with(workspace))
}

fn mask_git_config(
    args: &mut Vec<String>,
    created: &mut BTreeSet<String>,
    empty_file: &Path,
    path: &Path,
) -> anyhow::Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    anyhow::ensure!(
        metadata.is_file(),
        "Git configuration must be a regular file inside the sandbox"
    );
    bind_readonly_file(args, created, empty_file, path);
    Ok(())
}
