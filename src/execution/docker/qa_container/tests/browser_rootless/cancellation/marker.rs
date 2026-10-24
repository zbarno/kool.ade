use bollard::{
    container::LogOutput,
    exec::{CreateExecOptions, StartExecOptions, StartExecResults},
};
use futures_util::TryStreamExt;
use std::time::Duration;

const CANCEL_MARKER_WAIT: Duration = Duration::from_secs(60);
const MARKER_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const MARKER_PROBE: &str = "fetch('http://qa-task:4173/__cancel_started',{signal:AbortSignal.timeout(1000)}).then(async response=>process.stdout.write((await response.text()).trim())).catch(()=>process.exit(2))";

pub(super) async fn wait_for_cancel_marker(
    docker: &bollard::Docker,
    relay_container_id: &str,
    relay_runtime_user: &str,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + CANCEL_MARKER_WAIT;
    while tokio::time::Instant::now() < deadline {
        let probe = tokio::time::timeout(
            MARKER_PROBE_TIMEOUT,
            cancel_marker_is_present(docker, relay_container_id, relay_runtime_user),
        )
        .await;
        match probe {
            Ok(Ok(true)) => return Ok(()),
            Ok(Ok(false)) => {}
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err("timed out probing the browser cancellation marker".into()),
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err("browser never reached the delayed task page before cancellation".into())
}

async fn cancel_marker_is_present(
    docker: &bollard::Docker,
    relay_container_id: &str,
    relay_runtime_user: &str,
) -> Result<bool, String> {
    let exec = docker
        .create_exec(
            relay_container_id,
            CreateExecOptions::<String> {
                attach_stdin: Some(false),
                attach_stdout: Some(true),
                attach_stderr: Some(false),
                cmd: Some(vec!["node".into(), "-e".into(), MARKER_PROBE.into()]),
                user: Some(relay_runtime_user.into()),
                privileged: Some(false),
                ..Default::default()
            },
        )
        .await
        .map_err(|_| "could not start the read-only browser cancellation probe".to_string())?;
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
        .map_err(|_| "could not attach to the browser cancellation probe".to_string())?
    else {
        return Err("browser cancellation probe did not attach".into());
    };
    let mut stdout = Vec::new();
    while let Some(output) = output
        .try_next()
        .await
        .map_err(|_| "browser cancellation probe output could not be read".to_string())?
    {
        if let LogOutput::StdOut { message } | LogOutput::Console { message } = output {
            stdout.extend_from_slice(&message);
            if stdout.len() > 32 {
                return Err("browser cancellation probe output exceeded its bound".into());
            }
        }
    }
    let status = docker
        .inspect_exec(&exec.id)
        .await
        .map_err(|_| "browser cancellation probe result could not be inspected".to_string())?;
    Ok(status.exit_code == Some(0) && stdout == b"started")
}
