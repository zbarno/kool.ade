//! Read-only planning boundary for Pi's built-in read/search tools.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use super::{config::locate_bwrap, mounts};

pub(crate) struct PlanningSandbox {
    pub bwrap: PathBuf,
    pub args: Vec<String>,
    provider: Option<super::provider_bridge::ProviderBridge>,
}

impl PlanningSandbox {
    pub fn new(root: &Path, pi_executable: &Path) -> anyhow::Result<Self> {
        anyhow::ensure!(
            cfg!(target_os = "linux"),
            "Planning reads are paused because this platform has no configured filesystem sandbox"
        );
        let root = root.canonicalize()?;
        anyhow::ensure!(root.is_dir(), "Planning root is not a directory");
        let pi_executable = pi_executable.canonicalize()?;
        let repositories = registered_roots(&root)?;
        let mut sandbox = Self::build(&root, &pi_executable, repositories)?;
        if is_pi_cli(&pi_executable) {
            let provider = super::provider_bridge::ProviderBridge::start()?;
            sandbox.args.extend(provider.sandbox_mounts());
            sandbox.provider = Some(provider);
        }
        Ok(sandbox)
    }

    pub(super) fn build(
        root: &Path,
        pi_executable: &Path,
        repositories: Vec<PathBuf>,
    ) -> anyhow::Result<Self> {
        let bwrap = locate_bwrap(root)?;
        let args = planning_arguments(root, pi_executable, &repositories)?;
        Ok(Self {
            bwrap,
            args,
            provider: None,
        })
    }

    pub fn command_args(&self, command: &[String]) -> Vec<String> {
        let mut args = self.args.clone();
        let mut command = command.to_vec();
        if let Some(provider) = &self.provider {
            command.splice(1..1, provider.model_args.iter().cloned());
            args.extend([
                "--".into(),
                "/bin/sh".into(),
                "-c".into(),
                super::provider_bridge::BOOTSTRAP.into(),
                "koolade-planning".into(),
                provider.port.to_string(),
            ]);
            args.extend(command);
            return args;
        }
        args.push("--".into());
        args.extend(command);
        args
    }
}

fn is_pi_cli(executable: &Path) -> bool {
    executable.ancestors().any(|directory| {
        std::fs::read(directory.join("package.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|package| package["name"].as_str().map(str::to_owned))
            .is_some_and(|name| name == "@earendil-works/pi-coding-agent")
    })
}

fn planning_arguments(
    root: &Path,
    pi_executable: &Path,
    repositories: &[PathBuf],
) -> anyhow::Result<Vec<String>> {
    use mounts::{bind_readonly, bind_readonly_file, make_dir, mount_tmpfs, push_env};

    let mut args = vec![
        "--die-with-parent".into(),
        "--unshare-user".into(),
        "--unshare-pid".into(),
        "--unshare-net".into(),
        "--unshare-ipc".into(),
        "--unshare-uts".into(),
    ];
    let mut created = BTreeSet::from(["/".to_owned()]);
    // Construct a small runtime root instead of cloning the host filesystem.
    // System executables/libraries are read-only; project repositories are
    // mounted below as the only project data roots.
    for path in ["/usr", "/bin", "/sbin", "/lib", "/lib64"] {
        if Path::new(path).exists() {
            bind_readonly(&mut args, &mut created, Path::new(path), Path::new(path))?;
        }
    }
    if Path::new("/usr/local/src").is_dir() {
        mount_tmpfs(
            &mut args,
            &mut created,
            Path::new("/usr/local/src"),
            16_777_216,
        );
    }
    for path in [
        "/home", "/root", "/mnt", "/media", "/run", "/tmp", "/var", "/srv",
    ] {
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
    make_dir(&mut args, &mut created, Path::new("/etc"));
    for path in [
        "/etc/ld.so.cache",
        "/etc/passwd",
        "/etc/group",
        "/etc/nsswitch.conf",
        "/etc/localtime",
    ] {
        if Path::new(path).is_file() {
            bind_readonly_file(&mut args, &mut created, Path::new(path), Path::new(path));
        }
    }
    if Path::new("/etc/ssl/certs").is_dir() {
        bind_readonly(
            &mut args,
            &mut created,
            Path::new("/etc/ssl/certs"),
            Path::new("/etc/ssl/certs"),
        )?;
    }
    make_dir(&mut args, &mut created, Path::new("/proc"));
    args.extend(["--proc".into(), "/proc".into()]);
    make_dir(&mut args, &mut created, Path::new("/dev"));
    args.extend(["--dev".into(), "/dev".into()]);

    for repository in repositories {
        bind_readonly(&mut args, &mut created, repository, repository)?;
    }
    mount_pi_install(&mut args, &mut created, pi_executable)?;
    make_dir(&mut args, &mut created, Path::new("/tmp/koolade-home"));
    args.extend(["--chdir".into(), root.to_string_lossy().into_owned()]);
    args.push("--clearenv".into());
    push_env(&mut args, "HOME", "/tmp/koolade-home");
    push_env(&mut args, "TMPDIR", "/tmp");
    push_env(
        &mut args,
        "PATH",
        "/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin",
    );
    push_env(&mut args, "GIT_CONFIG_NOSYSTEM", "1");
    push_env(&mut args, "GIT_CONFIG_GLOBAL", "/dev/null");
    push_env(&mut args, "GIT_OPTIONAL_LOCKS", "0");
    push_env(&mut args, "GIT_TERMINAL_PROMPT", "0");
    Ok(args)
}

fn registered_roots(root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let manifest = crate::core::project_repos::ProjectManifest::load(root)?;
    let mut roots = vec![root.to_path_buf()];
    for repository in &manifest.repositories {
        if repository.id == "root" {
            continue;
        }
        match manifest.target_if_available(root, &repository.id)? {
            Some(path) if !roots.contains(&path) => roots.push(path),
            _ => {}
        }
    }
    anyhow::ensure!(
        roots.contains(&root.to_path_buf()),
        "Planning root is not registered"
    );
    Ok(roots)
}

pub(super) fn mount_pi_install(
    args: &mut Vec<String>,
    created: &mut BTreeSet<String>,
    executable: &Path,
) -> anyhow::Result<()> {
    let home = std::env::var_os("HOME").and_then(|path| PathBuf::from(path).canonicalize().ok());
    mount_selected_pi(args, created, executable, home.as_deref())
}

fn mount_selected_pi(
    args: &mut Vec<String>,
    created: &mut BTreeSet<String>,
    executable: &Path,
    home: Option<&Path>,
) -> anyhow::Result<()> {
    if let Some(home) = home
        && executable.starts_with(home)
        && let Some(package) = executable
            .ancestors()
            .find(|parent| parent.starts_with(home) && parent.join("package.json").is_file())
    {
        // npm installs commonly put credentials or unrelated tools elsewhere
        // in ~/.npm-global. Bind only Pi's package and its bundled dependencies.
        mounts::bind_readonly(args, created, package, package)?;
    } else {
        // An explicitly selected executable outside a package installation is
        // a required runtime binary; expose the file without its parent tree.
        mounts::bind_readonly_file(args, created, executable, executable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_install_mount_does_not_expose_neighboring_global_packages_or_credentials() {
        let root = std::env::temp_dir().join(format!(
            "koolade-pi-mount-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        let home = root.join("home");
        let package = home.join(".npm-global/lib/node_modules/pi-agent");
        std::fs::create_dir_all(&package).unwrap();
        let executable = package.join("dist/cli.js");
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        std::fs::write(&executable, "trusted Pi stub").unwrap();
        std::fs::write(package.join("package.json"), "{}").unwrap();
        std::fs::write(home.join(".npm-global/.npmrc"), "credential sentinel").unwrap();
        let mut args = Vec::new();
        let mut created = BTreeSet::from(["/".to_owned()]);
        mount_selected_pi(&mut args, &mut created, &executable, Some(&home)).unwrap();
        assert!(args.iter().any(|arg| arg == package.to_str().unwrap()));
        let global_root = home.join(".npm-global");
        assert!(
            !args.windows(3).any(|mount| {
                mount[0] == "--ro-bind" && mount[1] == global_root.to_string_lossy()
            })
        );
        assert!(
            !args
                .windows(3)
                .any(|mount| mount[0] == "--ro-bind" && mount[1].ends_with(".npmrc"))
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
