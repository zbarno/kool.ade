use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use super::mounts::{
    bind_readonly_file, bind_readwrite, make_dir, mount_tmpfs, mount_toolchains, push_env,
};
mod git;
mod path_safety;
pub(super) use git::{git_path, locate_git, validate_koolade_worktree};
use path_safety::inside_workspace;

pub(super) fn locate_bwrap(root: &Path) -> anyhow::Result<PathBuf> {
    if let Some(path) = std::env::var_os("KOOLADE_BWRAP_BIN") {
        let path = PathBuf::from(path).canonicalize()?;
        anyhow::ensure!(
            is_executable(&path),
            "KOOLADE_BWRAP_BIN must name an executable bubblewrap binary"
        );
        anyhow::ensure!(
            !inside_workspace(&path, root),
            "KOOLADE_BWRAP_BIN must not point inside a repository or task worktree"
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
    for path in ["/etc/npmrc", "/usr/etc/npmrc", "/usr/local/etc/npmrc"] {
        if Path::new(path).is_file() {
            bind_readonly_file(&mut args, &mut created, empty_file, Path::new(path));
        }
    }
    make_dir(&mut args, &mut created, Path::new("/proc"));
    args.extend(["--proc".into(), "/proc".into()]);
    make_dir(&mut args, &mut created, Path::new("/dev"));
    args.extend(["--dev".into(), "/dev".into()]);
    let has_rustup = mount_toolchains(&mut args, &mut created)?;
    let components =
        super::components::RuntimeComponents::mount(&mut args, &mut created, empty_file)?;
    bind_readwrite(&mut args, &mut created, root, root)?;
    let common_dir = git::mount_git_metadata(&mut args, &mut created, root, empty_file)?;
    if let Some(executable) = pi_executable {
        super::planning::mount_pi_install(&mut args, &mut created, executable)?;
    }
    make_dir(&mut args, &mut created, Path::new("/tmp/koolade-home"));
    bind_readonly_file(
        &mut args,
        &mut created,
        empty_file,
        Path::new("/tmp/koolade-home/.npm-globalrc"),
    );
    args.extend(["--chdir".into(), root.to_string_lossy().into_owned()]);
    args.push("--clearenv".into());
    push_env(&mut args, "HOME", "/tmp/koolade-home");
    push_env(&mut args, "TMPDIR", "/tmp");
    components.set_environment(
        &mut args,
        "/tmp/koolade-tools/cargo-bin:/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin",
    );
    push_env(&mut args, "KOOLADE_WORKTREE", &root.to_string_lossy());
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
    push_env(&mut args, "CARGO_HOME", "/tmp/koolade-tools/cargo-home");
    if has_rustup {
        push_env(&mut args, "RUSTUP_HOME", "/tmp/koolade-tools/rustup-home");
    }
    super::runtime_config::mount(&mut args, root)?;
    Ok((args, common_dir))
}
