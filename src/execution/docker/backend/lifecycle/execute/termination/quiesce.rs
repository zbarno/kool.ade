use crate::execution::{docker::backend::DockerExecutionBackend, errors::ExecutionError};
use bollard::exec::{CreateExecOptions, StartExecOptions, StartExecResults};
use futures_util::StreamExt;
use std::time::Duration;

// Do not signal PID 1 or its main child: Tini exits when the keeper dies.
// Identify that child by its app-owned argv marker, then freeze task processes
// before killing them. Enumeration is confined to this container's PID namespace.
const SCRIPT: &str = r#"
set -eu
marker=$0
keeper=''
for status in /proc/[0-9]*/stat; do
    pid=${status#/proc/}; pid=${pid%/stat}
    [ "$pid" != "$$" ] || continue
    read -r line < "$status" || continue
    fields=${line##*) }
    set -- $fields
    [ "$2" = 1 ] || continue
    if tr '\000' '\n' < "/proc/$pid/cmdline" | grep -Fxq -- "$marker"; then
        [ -z "$keeper" ] || exit 70
        keeper=$pid
    fi
done
[ -n "$keeper" ] || exit 71
pass=0
while [ "$pass" -lt 20 ]; do
    pass=$((pass + 1))
    targets=''
    for status in /proc/[0-9]*/stat; do
        pid=${status#/proc/}; pid=${pid%/stat}
        case "$pid" in 1|"$keeper"|"$$") continue;; esac
        read -r line < "$status" || continue
        fields=${line##*) }; set -- $fields
        [ "$1" != Z ] || continue
        [ "$2" != "$keeper" ] || continue
        [ "$2" != "$$" ] || continue
        kill -STOP "$pid" 2>/dev/null || continue
        targets="$targets $pid"
    done
    [ -n "$targets" ] || exit 0
    for pid in $targets; do kill -KILL "$pid" 2>/dev/null || :; done
    sleep 0.05
done
exit 72
"#;

pub(super) async fn stop_tasks(
    backend: &DockerExecutionBackend,
    container_id: &str,
    execution_id: &str,
) -> Result<(), ExecutionError> {
    // This helper runs only in the verified execution's private PID namespace.
    // Preserve both Docker init and its explicitly marked keeper child.
    let exec = backend
        .docker
        .create_exec(
            container_id,
            CreateExecOptions::<String> {
                attach_stdin: Some(false),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                cmd: Some(vec![
                    "/bin/sh".into(),
                    "-c".into(),
                    SCRIPT.into(),
                    format!("koolade-keeper-{execution_id}"),
                ]),
                user: Some("10001:10001".into()),
                privileged: Some(false),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| {
            super::super::state::operation_disconnected("stop task processes", &error)
        })?;
    let started = backend
        .docker
        .start_exec(
            &exec.id,
            Some(StartExecOptions {
                detach: false,
                tty: false,
                ..Default::default()
            }),
        )
        .await
        .map_err(|error| {
            super::super::state::operation_disconnected("stop task processes", &error)
        })?;
    let StartExecResults::Attached { mut output, .. } = started else {
        return Err(super::super::state::disconnected());
    };
    // Wait for shutdown itself to finish before creating the archive exec.
    // Inspection alone can observe an exec that has not started yet.
    while let Some(chunk) = output.next().await {
        chunk.map_err(|error| {
            super::super::state::operation_disconnected("wait for task shutdown", &error)
        })?;
    }
    loop {
        let status = backend
            .docker
            .inspect_exec(&exec.id)
            .await
            .map_err(|error| {
                super::super::state::operation_disconnected("inspect task shutdown", &error)
            })?;
        if status.running == Some(false) && status.exit_code.is_some() {
            return if status.exit_code == Some(0) {
                Ok(())
            } else {
                Err(super::super::state::disconnected())
            };
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}
