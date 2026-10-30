use bollard::{
    container::LogOutput,
    exec::{CreateExecOptions, StartExecOptions, StartExecResults},
};
use futures_util::TryStreamExt;

/// Execute file, socket, and environment probes as the locked user inside the
/// already-running rootless browser driver. The synthetic host files and the
/// host's real DOCKER_HOST socket must be unreachable from this container.
pub(super) async fn assert_host_resources_denied(
    docker: &bollard::Docker,
    container_id: &str,
    runtime_user: &str,
    host_identity_root: &std::path::Path,
) -> Result<(), String> {
    let socket_path = std::env::var("DOCKER_HOST")
        .ok()
        .and_then(|endpoint| endpoint.strip_prefix("unix://").map(str::to_owned))
        .filter(|path| std::path::Path::new(path).is_absolute())
        .ok_or_else(|| "rootless QA Docker socket path is unavailable to the test".to_owned())?;
    let checks = serde_json::json!({
        "files": [
            {"label":"synthetic ssh key", "path": host_identity_root.join(".ssh/id_rsa")},
            {"label":"synthetic cloud credentials", "path": host_identity_root.join(".aws/credentials")}
        ],
        "sockets": [
            {"label":"runner rootless Docker socket", "path": socket_path},
            {"label":"system Docker socket", "path":"/var/run/docker.sock"},
            {"label":"alternate Docker socket", "path":"/run/docker.sock"},
            {"label":"host X11 display", "path":"/tmp/.X11-unix/X0"},
            {"label":"host D-Bus", "path":"/run/dbus/system_bus_socket"}
        ],
        "environment": [
            "DOCKER_HOST", "DISPLAY", "WAYLAND_DISPLAY", "DBUS_SESSION_BUS_ADDRESS",
            "SSH_AUTH_SOCK", "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY",
            "ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GH_TOKEN", "GITHUB_TOKEN"
        ]
    });
    let checks = serde_json::to_string(&checks)
        .map_err(|_| "host-resource probe could not be encoded".to_owned())?;
    let script = r#"
const fs = require('node:fs/promises');
const net = require('node:net');
(async () => {
  const checks = JSON.parse(process.argv[1]);
  const leaked = [];
  for (const item of checks.files) {
    try { await fs.readFile(item.path); leaked.push(item.label); }
    catch (error) {
      if (!['ENOENT', 'EACCES', 'EPERM', 'ENOTDIR'].includes(error.code)) throw error;
    }
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
  for (const name of checks.environment) if (process.env[name]) leaked.push(`environment ${name}`);
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
