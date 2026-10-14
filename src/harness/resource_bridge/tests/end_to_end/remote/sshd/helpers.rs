use super::*;

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
            "Host fixture-host\n  HostName 127.0.0.1\n  Port {port}\n  IdentityFile {}\n  IdentitiesOnly yes\n  IdentityAgent none\n  BatchMode yes\n  StrictHostKeyChecking yes\n  UpdateHostKeys no\n  UserKnownHostsFile {}\n  GlobalKnownHostsFile /dev/null\n  HostKeyAlias fixture-worker\n  SendEnv KOOLADE_HOME DOCKER_HOST XDG_RUNTIME_DIR PATH\n",
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
