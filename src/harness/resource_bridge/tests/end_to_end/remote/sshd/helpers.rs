use super::*;

impl SshdFixture {
    pub(in crate::harness::resource_bridge::tests::end_to_end::remote) fn add_controller_known_host(
        &self,
        worker_id: &str,
    ) -> std::io::Result<()> {
        let directory = crate::persistence::state_root().join("remote-workers");
        fs::create_dir_all(&directory)?;
        let path = directory.join("known_hosts");
        let mut contents = fs::read_to_string(&path).unwrap_or_default();
        if !contents.is_empty() && !contents.ends_with('\n') {
            contents.push('\n');
        }
        contents.push_str(&format!(
            "{worker_id} {}\n",
            public_key_fields(&self.host_key_public)?
        ));
        fs::write(&path, contents)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}

pub(super) fn worker_state_command(
    root: &Path,
    worker_binary: &Path,
    worker_id: Option<&str>,
) -> std::io::Result<String> {
    let binary = shell_quote(&worker_binary.canonicalize()?.to_string_lossy());
    let Some(worker_id) = worker_id else {
        return Ok(format!("{binary} worker serve --stdio"));
    };
    if !uuid::Uuid::parse_str(worker_id).is_ok_and(|id| id.hyphenated().to_string() == worker_id) {
        return Err(std::io::Error::other("test worker identity is invalid"));
    }
    let state = root.join("worker-state");
    let executions = state.join("executions");
    fs::create_dir_all(&executions)?;
    fs::write(executions.join("owner-instance-id"), worker_id)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(&executions, fs::Permissions::from_mode(0o700))?;
        fs::set_permissions(
            executions.join("owner-instance-id"),
            fs::Permissions::from_mode(0o600),
        )?;
    }
    Ok(format!(
        "/usr/bin/env KOOLADE_HOME={} {binary} worker serve --stdio",
        shell_quote(&state.to_string_lossy())
    ))
}

pub(in crate::harness::resource_bridge::tests::end_to_end::remote) fn find_sshd() -> Option<PathBuf>
{
    ["sshd", "/usr/sbin/sshd", "/usr/local/sbin/sshd"]
        .into_iter()
        .find_map(|candidate| {
            let path = PathBuf::from(candidate);
            if path.is_file() { Some(path) } else { None }
        })
}

pub(in crate::harness::resource_bridge::tests::end_to_end::remote) fn generate_key(
    path: &Path,
) -> std::io::Result<()> {
    let status = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(path)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            "ssh-keygen failed to create fixture key",
        ))
    }
}

pub(in crate::harness::resource_bridge::tests::end_to_end::remote) fn public_key_fields(
    path: &Path,
) -> std::io::Result<String> {
    let key = fs::read_to_string(path)?;
    let mut fields = key.split_whitespace();
    let kind = fields
        .next()
        .ok_or_else(|| std::io::Error::other("SSH public-key type is missing"))?;
    let body = fields
        .next()
        .ok_or_else(|| std::io::Error::other("SSH public-key body is missing"))?;
    Ok(format!("{kind} {body}"))
}

pub(in crate::harness::resource_bridge::tests::end_to_end::remote) fn write_client_config(
    path: &Path,
    port: u16,
    identity: &Path,
) -> std::io::Result<()> {
    fs::write(
        path,
        format!(
            "Host fixture-host\n  HostName 127.0.0.1\n  Port {port}\n  IdentityFile {}\n  IdentitiesOnly yes\n  IdentityAgent none\n  BatchMode yes\n  StrictHostKeyChecking yes\n  UpdateHostKeys no\n  UserKnownHostsFile {}\n  GlobalKnownHostsFile /dev/null\n  HostKeyAlias fixture-worker\n  SendEnv KOOLADE_HOME DOCKER_HOST XDG_RUNTIME_DIR\n",
            identity.display(),
            path.parent().unwrap().join("known_hosts").display()
        ),
    )
}

pub(in crate::harness::resource_bridge::tests::end_to_end::remote) fn write_wrapper(
    root: &Path,
    name: &str,
    config: &Path,
) -> std::io::Result<PathBuf> {
    let wrapper = root.join(name);
    let ssh = find_ssh()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "ssh was not found"))?;
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nexec {} -F {} \"$@\"\n",
            shell_quote(&ssh.to_string_lossy()),
            shell_quote(&config.to_string_lossy())
        ),
    )?;
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o700))?;
    Ok(wrapper)
}

pub(in crate::harness::resource_bridge::tests::end_to_end::remote) fn find_ssh() -> Option<PathBuf>
{
    ["/usr/bin/ssh", "/bin/ssh"]
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

pub(in crate::harness::resource_bridge::tests::end_to_end::remote) fn command_text(
    program: &str,
    args: &[&str],
) -> std::io::Result<String> {
    let output = Command::new(program).args(args).output()?;
    if !output.status.success() {
        return Err(std::io::Error::other(format!("{program} failed")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(in crate::harness::resource_bridge::tests::end_to_end::remote) fn shell_quote(
    value: &str,
) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_client_config_uses_the_pinned_known_hosts_alias() {
        let root =
            std::env::temp_dir().join(format!("koolade-ssh-config-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let config = root.join("ssh_config");
        let identity = root.join("identity");
        write_client_config(&config, 22_222, &identity).unwrap();

        let output = Command::new(find_ssh().unwrap())
            .args(["-G", "-F"])
            .arg(&config)
            .arg("fixture-host")
            .output()
            .unwrap();
        assert!(output.status.success());
        let resolved = String::from_utf8_lossy(&output.stdout);
        assert!(
            resolved
                .lines()
                .any(|line| line == "hostkeyalias fixture-worker")
        );
        assert!(resolved.lines().any(|line| {
            line == format!("userknownhostsfile {}", root.join("known_hosts").display())
        }));
        let _ = fs::remove_dir_all(root);
    }
}
