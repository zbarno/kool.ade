use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const GO_FETCH_TIMEOUT: Duration = Duration::from_secs(7 * 60);
const GO_PROXY_HOSTS: &[&str] = &[
    "proxy.golang.org",
    "sum.golang.org",
    "storage.googleapis.com",
];

pub(super) struct FetchOutcome {
    pub(super) bytes_received: usize,
    pub(super) result: anyhow::Result<()>,
}

pub(super) fn fetch_from_proxy(
    worktree: &Path,
    workspace: &Path,
    gopath: &Path,
    reservation: usize,
) -> FetchOutcome {
    let proxy = match crate::harness::resource_bridge::proxy::HttpsRegistryProxy::start(
        GO_PROXY_HOSTS.iter().map(|host| (*host).to_owned()),
        reservation,
    ) {
        Ok(proxy) => proxy,
        Err(error) => {
            return FetchOutcome {
                bytes_received: 0,
                result: Err(error),
            };
        }
    };
    let result = locate_go(worktree).and_then(|go| {
        run_go_command(
            &go,
            workspace,
            gopath,
            "https://proxy.golang.org",
            Some(&proxy.url()),
            ["mod", "download", "all"],
        )
    });
    FetchOutcome {
        bytes_received: proxy.bytes_received(),
        result,
    }
}

pub(super) fn verify_offline(
    worktree: &Path,
    workspace: &Path,
    gopath: &Path,
) -> anyhow::Result<()> {
    let go = locate_go(worktree)?;
    run_go_command(&go, workspace, gopath, "off", None, ["mod", "verify"])
}

pub(super) fn verify_existing_cache(
    worktree: &Path,
    gopath: &Path,
    expected_identity: &str,
) -> anyhow::Result<()> {
    let stage = gopath.join(format!(".verify-{}", uuid::Uuid::new_v4().simple()));
    fs::create_dir(&stage)?;
    let result = (|| {
        super::copy_lockfile(worktree, &stage, "go.mod")?;
        super::copy_lockfile(worktree, &stage, "go.sum")?;
        verify_lock_identity(&stage, expected_identity)?;
        let go = locate_go(worktree)?;
        run_go_command(&go, &stage, gopath, "off", None, ["mod", "download", "all"])?;
        run_go_command(&go, &stage, gopath, "off", None, ["mod", "verify"])?;
        verify_lock_identity(&stage, expected_identity)
    })();
    let _ = fs::remove_dir_all(stage);
    result
}

fn verify_lock_identity(worktree: &Path, expected_identity: &str) -> anyhow::Result<()> {
    let observed_identity = super::lockfile::identity(&super::lockfile::collect(worktree)?)?;
    anyhow::ensure!(
        observed_identity == expected_identity,
        "Go lock inputs changed during existing-cache verification"
    );
    Ok(())
}

fn run_go_command<const N: usize>(
    go: &Path,
    workspace: &Path,
    gopath: &Path,
    goproxy: &str,
    http_proxy: Option<&str>,
    arguments: [&str; N],
) -> anyhow::Result<()> {
    let mut command = configured_go_command(go, workspace, gopath, goproxy, http_proxy)?;
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn()?;
    let deadline = Instant::now() + GO_FETCH_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::ensure!(
                status.success(),
                "Host Go could not verify the pinned go.sum modules (exit {status})"
            );
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("Go module preparation exceeded seven minutes");
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn configured_go_command(
    go: &Path,
    workspace: &Path,
    gopath: &Path,
    goproxy: &str,
    http_proxy: Option<&str>,
) -> anyhow::Result<Command> {
    let home = workspace.join(".broker-home");
    fs::create_dir_all(&home)?;
    let mod_cache = gopath.join("pkg/mod");
    fs::create_dir_all(&mod_cache)?;
    let mut command = Command::new(go);
    command
        .current_dir(workspace)
        .env_clear()
        .env("PATH", "/usr/local/go/bin:/usr/local/bin:/usr/bin:/bin")
        .env("HOME", &home)
        .env("GOPATH", gopath)
        .env("GOMODCACHE", &mod_cache)
        .env("GOPROXY", goproxy)
        .env(
            "GOSUMDB",
            if goproxy == "off" {
                "off"
            } else {
                "sum.golang.org"
            },
        )
        .env("GOTOOLCHAIN", "local")
        .env("GOENV", "off")
        .env("GOWORK", "off")
        .env("GOVCS", "*:off")
        .env("GOPRIVATE", "")
        .env("GONOPROXY", "")
        .env("GONOSUMDB", "")
        .env("GOINSECURE", "");
    if let Some(proxy) = http_proxy {
        command
            .env("HTTPS_PROXY", proxy)
            .env("https_proxy", proxy)
            .env("HTTP_PROXY", proxy)
            .env("http_proxy", proxy)
            .env("NO_PROXY", "")
            .env("no_proxy", "");
    }
    Ok(command)
}

fn locate_go(worktree: &Path) -> anyhow::Result<PathBuf> {
    let path = env::var_os("PATH")
        .ok_or_else(|| anyhow::anyhow!("Host PATH is unavailable for Go preparation"))?;
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    locate_go_in_path(worktree, &path, home.as_deref())
}

fn locate_go_in_path(
    worktree: &Path,
    path: &std::ffi::OsStr,
    home: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    let worktree = worktree.canonicalize()?;
    for directory in env::split_paths(path).filter(|directory| directory.is_absolute()) {
        let Ok(directory) = directory.canonicalize() else {
            continue;
        };
        let candidate = directory.join("go");
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        let metadata = canonical.metadata()?;
        let trusted = ["/usr", "/bin", "/usr/local"]
            .iter()
            .any(|root| directory.starts_with(root) && canonical.starts_with(root))
            || home.is_some_and(|home| {
                directory.starts_with(home.join("go/bin"))
                    && canonical.starts_with(home.join("go/bin"))
            });
        if metadata.is_file()
            && metadata.permissions().mode() & 0o111 != 0
            && !canonical.starts_with(&worktree)
            && trusted
        {
            return Ok(candidate);
        }
    }
    anyhow::bail!("Host Go is unavailable; install a trusted Go toolchain and resume the task")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environment(command: &Command) -> std::collections::BTreeMap<String, String> {
        command
            .get_envs()
            .filter_map(|(key, value)| {
                Some((key.to_str()?.to_owned(), value?.to_str()?.to_owned()))
            })
            .collect()
    }

    #[test]
    fn broker_go_environment_disables_private_configuration_vcs_and_toolchain_downloads() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let gopath = root.path().join("go");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&gopath).unwrap();
        let command = configured_go_command(
            Path::new("/usr/local/go/bin/go"),
            &workspace,
            &gopath,
            "https://proxy.golang.org",
            Some("http://127.0.0.1:4321"),
        )
        .unwrap();
        let environment = environment(&command);
        for (name, value) in [
            ("GOPROXY", "https://proxy.golang.org"),
            ("GOSUMDB", "sum.golang.org"),
            ("GOTOOLCHAIN", "local"),
            ("GOENV", "off"),
            ("GOWORK", "off"),
            ("GOVCS", "*:off"),
            ("GOPRIVATE", ""),
            ("GONOPROXY", ""),
            ("GONOSUMDB", ""),
        ] {
            assert_eq!(environment.get(name).map(String::as_str), Some(value));
        }
        assert_eq!(
            environment.get("HTTPS_PROXY").map(String::as_str),
            Some("http://127.0.0.1:4321")
        );
        assert!(!environment.contains_key("GIT_CONFIG_COUNT"));
        assert!(!environment.contains_key("GITHUB_TOKEN"));
    }

    #[test]
    fn offline_go_environment_has_no_proxy_route() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let gopath = root.path().join("go");
        fs::create_dir(&workspace).unwrap();
        fs::create_dir(&gopath).unwrap();
        let command = configured_go_command(
            Path::new("/usr/local/go/bin/go"),
            &workspace,
            &gopath,
            "off",
            None,
        )
        .unwrap();
        let environment = environment(&command);
        assert_eq!(environment.get("GOPROXY").map(String::as_str), Some("off"));
        assert_eq!(environment.get("GOSUMDB").map(String::as_str), Some("off"));
        assert!(!environment.contains_key("HTTPS_PROXY"));
        assert!(!environment.contains_key("HTTP_PROXY"));
    }
}
