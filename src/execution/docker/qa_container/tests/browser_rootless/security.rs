use bollard::{
    container::LogOutput,
    exec::{CreateExecOptions, StartExecOptions, StartExecResults},
};
use futures_util::TryStreamExt;
use tempfile::TempDir;

pub(crate) fn synthetic_host_identity_root() -> TempDir {
    let home = std::path::PathBuf::from(std::env::var_os("HOME").expect("runner HOME is set"));
    let root = tempfile::Builder::new()
        .prefix("koolade-rootless-host-identity-")
        .tempdir_in(home)
        .unwrap();
    std::fs::create_dir_all(root.path().join(".ssh")).unwrap();
    std::fs::write(
        root.path().join(".ssh/id_rsa"),
        b"synthetic-host-ssh-key-must-not-be-readable-from-qa",
    )
    .unwrap();
    std::fs::create_dir_all(root.path().join(".aws")).unwrap();
    std::fs::write(
        root.path().join(".aws/credentials"),
        b"[default]\naws_access_key_id=synthetic-host-key\n",
    )
    .unwrap();
    root
}

fn local_x11_display_socket(display: &str) -> Option<String> {
    let display = display.strip_prefix("unix/").unwrap_or(display);
    let number = display.rsplit_once(':')?.1.split('.').next()?;
    number
        .parse::<u16>()
        .ok()
        .map(|number| format!("/tmp/.X11-unix/X{number}"))
}

#[cfg(unix)]
fn host_x11_socket_probe(label: &str, path: &str) -> Option<serde_json::Value> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_socket() {
        return None;
    }
    Some(serde_json::json!({
        "label": label,
        "path": path,
        "host_identity": {
            "device": metadata.dev().to_string(),
            "inode": metadata.ino().to_string()
        }
    }))
}

#[cfg(not(unix))]
fn host_x11_socket_probe(_label: &str, _path: &str) -> Option<serde_json::Value> {
    None
}

/// Execute file, system-write, socket, and environment probes as the locked
/// user inside the already-running rootless QA driver. The synthetic host
/// files and the host's real DOCKER_HOST socket must be unreachable, and a
/// package-install marker must not be creatable in the immutable system path.
/// The host DISPLAY socket is checked independently of the private display
/// configured inside a desktop candidate container.
/// The live write attempt proves effective filesystem denial beyond inspecting
/// Docker's read-only-root setting. Dependencies: rootless Docker and the
/// candidate browser or desktop image already used by the enclosing test.
/// Setup cost: one extra failed file creation in that existing container.
pub(crate) async fn assert_host_resources_denied(
    docker: &bollard::Docker,
    container_id: &str,
    runtime_user: &str,
    host_identity_root: &std::path::Path,
    container_display: Option<&str>,
) -> Result<(), String> {
    let socket_path = std::env::var("DOCKER_HOST")
        .ok()
        .and_then(|endpoint| endpoint.strip_prefix("unix://").map(str::to_owned))
        .filter(|path| std::path::Path::new(path).is_absolute())
        .ok_or_else(|| "rootless QA Docker socket path is unavailable to the test".to_owned())?;
    let mut sockets = vec![
        serde_json::json!({"label":"runner rootless Docker socket", "path":socket_path}),
        serde_json::json!({"label":"system Docker socket", "path":"/var/run/docker.sock"}),
        serde_json::json!({"label":"alternate Docker socket", "path":"/run/docker.sock"}),
        serde_json::json!({"label":"host D-Bus", "path":"/run/dbus/system_bus_socket"}),
    ];
    if let Some(probe) = host_x11_socket_probe("host X11 display", "/tmp/.X11-unix/X0") {
        sockets.push(probe);
    }
    if let Some(socket) = std::env::var_os("DISPLAY")
        .and_then(|value| value.into_string().ok())
        .and_then(|display| local_x11_display_socket(&display))
        && let Some(probe) = host_x11_socket_probe("runner X11 display", &socket)
    {
        sockets.push(probe);
    }
    if let Some(wayland) =
        std::env::var_os("WAYLAND_DISPLAY").and_then(|value| value.into_string().ok())
    {
        let path = if std::path::Path::new(&wayland).is_absolute() {
            Some(wayland)
        } else {
            std::env::var_os("XDG_RUNTIME_DIR")
                .map(std::path::PathBuf::from)
                .map(|root| root.join(wayland).display().to_string())
        };
        if let Some(path) = path {
            sockets.push(serde_json::json!({"label":"runner Wayland display", "path":path}));
        }
    }
    let mut environment = serde_json::Map::new();
    for name in [
        "DOCKER_HOST",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "DBUS_SESSION_BUS_ADDRESS",
        "SSH_AUTH_SOCK",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "GH_TOKEN",
        "GITHUB_TOKEN",
    ] {
        environment.insert(name.into(), serde_json::Value::Null);
    }
    if let Some(display) = container_display {
        environment.insert("DISPLAY".into(), display.into());
    }
    let checks = serde_json::json!({
        "files": [
            {"label":"synthetic ssh key", "path": host_identity_root.join(".ssh/id_rsa")},
            {"label":"synthetic cloud credentials", "path": host_identity_root.join(".aws/credentials")}
        ],
        "system_files": [
            {"label":"runtime QA system executable directory", "path":"/usr/local/bin"}
        ],
        "sockets": sockets,
        "environment": environment
    });
    let checks = serde_json::to_string(&checks)
        .map_err(|_| "host-resource probe could not be encoded".to_owned())?;
    let script = r#"
const fs = require('node:fs/promises');
const net = require('node:net');
const { spawnSync } = require('node:child_process');
(async () => {
  const checks = JSON.parse(process.argv[1]);
  const leaked = [];
  for (const item of checks.files) {
    try { await fs.readFile(item.path); leaked.push(item.label); }
    catch (error) {
      if (!['ENOENT', 'EACCES', 'EPERM', 'ENOTDIR'].includes(error.code)) throw error;
    }
  }
  for (const item of checks.system_files) {
    const path = `${item.path}/.koolade-package-install-probe-${process.pid}`;
    let handle;
    try {
      handle = await fs.open(path, 'wx', 0o600);
      leaked.push(item.label);
    } catch (error) {
      if (!['EACCES', 'EPERM', 'EROFS'].includes(error.code)) throw error;
    } finally {
      await handle?.close();
      if (handle) await fs.rm(path, { force: true });
    }
  }
  const executable = `/tmp/.koolade-noexec-probe-${process.pid}`;
  try {
    await fs.writeFile(executable, '#!/bin/sh\nexit 0\n', { mode: 0o700 });
    const result = spawnSync(executable, [], { timeout: 1000 });
    if (!result.error || !['EACCES', 'EPERM'].includes(result.error.code)) {
      leaked.push('execution from writable tmpfs');
    }
  } finally {
    await fs.rm(executable, { force: true });
  }
  for (const item of checks.sockets) {
    let metadata;
    try { metadata = await fs.stat(item.path, { bigint: true }); }
    catch (error) {
      if (!['ENOENT', 'EACCES', 'EPERM', 'ENOTDIR'].includes(error.code)) throw error;
      continue;
    }
    if (item.host_identity && (
      String(metadata.dev) !== item.host_identity.device ||
      String(metadata.ino) !== item.host_identity.inode
    )) continue;
    await new Promise((resolve, reject) => {
      const socket = net.createConnection({ path: item.path });
      const timer = setTimeout(() => {
        socket.destroy();
        reject(new Error('socket probe timed out'));
      }, 1000);
      socket.once('connect', () => {
        clearTimeout(timer);
        leaked.push(item.label);
        socket.destroy();
        resolve();
      });
      socket.once('error', error => {
        clearTimeout(timer);
        if (['ENOENT', 'EACCES', 'EPERM', 'ENOTDIR', 'ECONNREFUSED'].includes(error.code)) resolve();
        else reject(error);
      });
    });
  }
  for (const [name, expected] of Object.entries(checks.environment)) {
    const actual = process.env[name];
    if (expected === null ? actual !== undefined && actual !== '' : actual !== expected) {
      leaked.push(`environment ${name}`);
    }
  }
  if (leaked.length) {
    process.stderr.write(`host resources were visible: ${leaked.join(', ')}`);
    process.exitCode = 1;
  } else {
    process.stdout.write('host-resource-denials-passed');
  }
})().catch(() => { process.stderr.write('host-resource probe failed'); process.exitCode = 1; });
"#;
    let exec = docker
        .create_exec(
            container_id,
            CreateExecOptions::<String> {
                attach_stdin: Some(false),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(vec!["node".into(), "-e".into(), script.into(), checks]),
                user: Some(runtime_user.into()),
                privileged: Some(false),
                ..Default::default()
            },
        )
        .await
        .map_err(|_| "host-resource probe could not start in the browser driver".to_owned())?;
    let StartExecResults::Attached { mut output, .. } = docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                output_capacity: Some(1024),
            }),
        )
        .await
        .map_err(|_| "host-resource probe output could not be attached".to_owned())?
    else {
        return Err("host-resource probe did not attach".into());
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    while let Some(output) = output
        .try_next()
        .await
        .map_err(|_| "host-resource probe output could not be read".to_owned())?
    {
        match output {
            LogOutput::StdOut { message } | LogOutput::Console { message } => {
                stdout.extend_from_slice(&message)
            }
            LogOutput::StdErr { message } => stderr.extend_from_slice(&message),
            LogOutput::StdIn { .. } => continue,
        }
        if stdout.len().saturating_add(stderr.len()) > 4096 {
            return Err("host-resource probe output exceeded its bound".into());
        }
    }
    let status = docker
        .inspect_exec(&exec.id)
        .await
        .map_err(|_| "host-resource probe result could not be inspected".to_owned())?;
    if status.exit_code != Some(0) || stdout != b"host-resource-denials-passed" {
        return Err("live browser container could access a forbidden host resource".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::local_x11_display_socket;

    #[test]
    fn host_x11_display_socket_parses_local_display_variants() {
        for (display, expected) in [
            (":0", Some("/tmp/.X11-unix/X0")),
            (":99.0", Some("/tmp/.X11-unix/X99")),
            ("unix/:99.1", Some("/tmp/.X11-unix/X99")),
            ("localhost:10.0", Some("/tmp/.X11-unix/X10")),
            ("not-a-display", None),
            (":bad", None),
        ] {
            assert_eq!(local_x11_display_socket(display).as_deref(), expected);
        }
    }
}
