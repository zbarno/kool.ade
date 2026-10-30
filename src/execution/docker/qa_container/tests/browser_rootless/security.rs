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

/// Execute file, socket, and environment probes as the locked user inside the
/// already-running rootless QA driver. The synthetic host files and the
/// host's real DOCKER_HOST socket must be unreachable from this container.
pub(crate) async fn assert_host_resources_denied(
    docker: &bollard::Docker,
    container_id: &str,
    runtime_user: &str,
    host_identity_root: &std::path::Path,
    allowed_display: Option<&str>,
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
        serde_json::json!({"label":"host X11 display", "path":"/tmp/.X11-unix/X0"}),
        serde_json::json!({"label":"host D-Bus", "path":"/run/dbus/system_bus_socket"}),
    ];
    if let Some(display) = std::env::var_os("DISPLAY").and_then(|value| value.into_string().ok())
        && let Some(number) = display
            .strip_prefix(':')
            .and_then(|value| value.parse::<u16>().ok())
        && allowed_display != Some(display.as_str())
    {
        sockets.push(serde_json::json!({
            "label":"runner X11 display",
            "path":format!("/tmp/.X11-unix/X{number}")
        }));
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
    if let Some(display) = allowed_display {
        environment.insert("DISPLAY".into(), display.into());
    }
    let checks = serde_json::json!({
        "files": [
            {"label":"synthetic ssh key", "path": host_identity_root.join(".ssh/id_rsa")},
            {"label":"synthetic cloud credentials", "path": host_identity_root.join(".aws/credentials")}
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
    let present = false;
    try { await fs.stat(item.path); present = true; }
    catch (error) {
      if (!['ENOENT', 'EACCES', 'EPERM', 'ENOTDIR'].includes(error.code)) throw error;
    }
    if (present) {
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
