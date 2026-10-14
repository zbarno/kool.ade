use super::{
    RemoteWorkerRegistration, create_private_dir, load_from, register_at, registration_path,
    valid_identifier,
};
use std::{fs, path::Path};

const KEYSCAN_TIMEOUT_SECONDS: &str = "8";

/// Verify a presented SSH fingerprint against a bounded host-key scan, then
/// persist only the matching public host key and worker registration.
pub fn trust(
    worker_id: &str,
    host_alias: &str,
    principal: &str,
    expected_fingerprint: &str,
) -> anyhow::Result<RemoteWorkerRegistration> {
    validate_fingerprint(expected_fingerprint)?;
    anyhow::ensure!(valid_identifier(worker_id), "Worker ID is invalid");
    anyhow::ensure!(valid_host_token(host_alias), "SSH host alias is invalid");
    anyhow::ensure!(valid_host_token(principal), "SSH principal is invalid");
    let (host, port) = resolve_ssh_target(host_alias, principal)?;
    let scan = std::process::Command::new("ssh-keyscan")
        .args([
            "-p",
            &port,
            "-T",
            KEYSCAN_TIMEOUT_SECONDS,
            "-t",
            "ed25519,ecdsa,rsa",
            &host,
        ])
        .output()?;
    anyhow::ensure!(
        scan.stdout.len() <= 64 * 1024,
        "SSH host-key response exceeded the size limit"
    );
    anyhow::ensure!(
        scan.status.success() || !scan.stdout.is_empty(),
        "Could not retrieve an SSH host key for this alias"
    );
    let key = matching_key(&String::from_utf8_lossy(&scan.stdout), expected_fingerprint)?;
    trust_at(
        &crate::persistence::state_root(),
        worker_id,
        host_alias,
        principal,
        expected_fingerprint,
        &key,
    )
}

fn resolve_ssh_target(alias: &str, principal: &str) -> anyhow::Result<(String, String)> {
    let output = std::process::Command::new("ssh")
        .args([
            "-G",
            "-o",
            "ProxyCommand=none",
            "-o",
            "ProxyJump=none",
            "-l",
            principal,
            alias,
        ])
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "Could not resolve the SSH alias using the local SSH configuration"
    );
    let text = String::from_utf8(output.stdout)?;
    let host = text
        .lines()
        .find_map(|line| line.strip_prefix("hostname "))
        .map(str::trim)
        .filter(|value| valid_network_host(value))
        .ok_or_else(|| anyhow::anyhow!("SSH alias resolved to an invalid host"))?;
    let port = text
        .lines()
        .find_map(|line| line.strip_prefix("port "))
        .map(str::trim)
        .filter(|value| value.parse::<u16>().is_ok_and(|port| port > 0))
        .ok_or_else(|| anyhow::anyhow!("SSH alias resolved to an invalid port"))?;
    Ok((host.to_owned(), port.to_owned()))
}

fn trust_at(
    root: &Path,
    worker_id: &str,
    host_alias: &str,
    principal: &str,
    fingerprint: &str,
    key: &str,
) -> anyhow::Result<RemoteWorkerRegistration> {
    let registration = RemoteWorkerRegistration {
        schema_version: 1,
        worker_id: worker_id.to_owned(),
        ssh_host_alias: host_alias.to_owned(),
        ssh_principal: principal.to_owned(),
        host_key_fingerprint: fingerprint.to_owned(),
        enrolled_at_unix_ms: crate::persistence::harness_settings::current_unix_ms(),
        enabled: true,
        max_jobs: 1,
        supported_architectures: Vec::new(),
        trusted_image_digests: Vec::new(),
        resource_limit: None,
        last_probe: None,
        policy_revision: String::new(),
    };
    registration.validate()?;
    anyhow::ensure!(
        load_from(root, worker_id)?.is_none(),
        "Worker ID is already enrolled; revoke it before rebinding"
    );
    let known_hosts = known_hosts_path(root);
    let previous = read_known_hosts(&known_hosts)?;
    anyhow::ensure!(
        !previous
            .lines()
            .any(|line| line.split_whitespace().next() == Some(worker_id)),
        "Worker ID already has a known-hosts pin"
    );
    let mut updated = previous.clone();
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&format!("{worker_id} {key}\n"));
    write_known_hosts(&known_hosts, &updated)?;
    if let Err(error) = register_at(root, registration.clone()) {
        let _ = restore_known_hosts(&known_hosts, &updated, &previous);
        return Err(error);
    }
    Ok(registration)
}

/// Remove a worker's registration and its stable-ID host-key pin.
pub fn revoke(worker_id: &str) -> anyhow::Result<()> {
    let root = crate::persistence::state_root();
    revoke_at(&root, worker_id)
}

fn revoke_at(root: &Path, worker_id: &str) -> anyhow::Result<()> {
    let registration = registration_path(root, worker_id)?;
    let _existing = load_from(root, worker_id)?
        .ok_or_else(|| anyhow::anyhow!("remote worker is not enrolled"))?;
    let known_hosts = known_hosts_path(root);
    let contents = read_known_hosts(&known_hosts)?;
    let updated = contents
        .lines()
        .filter(|line| line.split_whitespace().next() != Some(worker_id))
        .collect::<Vec<_>>()
        .join("\n");
    write_known_hosts(&known_hosts, &updated)?;
    fs::remove_file(registration)?;
    Ok(())
}

fn read_known_hosts(path: &Path) -> anyhow::Result<String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            anyhow::ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Worker known_hosts file is unsafe"
            );
            let content = fs::read_to_string(path)?;
            anyhow::ensure!(
                content.len() <= 1024 * 1024,
                "known_hosts file is too large"
            );
            Ok(content)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.into()),
    }
}

fn write_known_hosts(path: &Path, contents: &str) -> anyhow::Result<()> {
    create_private_dir(path.parent().expect("known_hosts path has a parent"))?;
    crate::artifacts::atomic_write_bytes(path, contents.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn restore_known_hosts(path: &Path, written: &str, previous: &str) -> anyhow::Result<()> {
    let current = read_known_hosts(path)?;
    if current == written {
        write_known_hosts(path, previous)?;
    }
    Ok(())
}

fn known_hosts_path(root: &Path) -> std::path::PathBuf {
    root.join("remote-workers").join("known_hosts")
}

fn matching_key(scan: &str, expected_fingerprint: &str) -> anyhow::Result<String> {
    let mut matches = Vec::new();
    for line in scan
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 3 {
            continue;
        }
        let temporary = std::env::temp_dir().join(format!(
            "koolade-ssh-key-fingerprint-{}.pub",
            uuid::Uuid::new_v4().simple()
        ));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        use std::io::Write as _;
        writeln!(file, "{} {}", fields[1], fields[2])?;
        drop(file);
        let output = std::process::Command::new("ssh-keygen")
            .args(["-lf"])
            .arg(&temporary)
            .output();
        let _ = fs::remove_file(&temporary);
        let output = output?;
        if output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .nth(1)
                == Some(expected_fingerprint)
        {
            matches.push(format!("{} {}", fields[1], fields[2]));
        }
    }
    matches.sort();
    matches.dedup();
    anyhow::ensure!(
        matches.len() == 1,
        "Scanned host keys did not contain exactly one key matching the trusted fingerprint"
    );
    Ok(matches.remove(0))
}

fn validate_fingerprint(value: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        value.starts_with("SHA256:")
            && value.len() == 50
            && value[7..]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/')),
        "Host-key fingerprint must use OpenSSH SHA256 format"
    );
    Ok(())
}

fn valid_host_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn valid_network_host(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

#[cfg(test)]
#[path = "enrollment/tests.rs"]
mod tests;
