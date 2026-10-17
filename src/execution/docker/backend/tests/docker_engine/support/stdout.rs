use std::time::Duration;

use tokio::task::JoinHandle;

use crate::execution::{
    backend::{ExecutionBackend, ExecutionHandleV1},
    contract::{EventPageRequestV1, ExecutionEventKindV1, ExecutionEventV1},
};

const STDOUT_WAIT: Duration = Duration::from_secs(60);

pub(crate) async fn wait_for_stdout(
    backend: &crate::execution::docker::backend::DockerExecutionBackend,
    handle: &ExecutionHandleV1,
    expected: &str,
) -> Vec<ExecutionEventV1> {
    wait_for_stdout_inner(backend, handle, expected, None).await
}

pub(crate) async fn wait_for_stdout_with_task(
    backend: &crate::execution::docker::backend::DockerExecutionBackend,
    handle: &ExecutionHandleV1,
    expected: &str,
    task: &JoinHandle<Result<(), crate::execution::errors::ExecutionError>>,
) -> Vec<ExecutionEventV1> {
    wait_for_stdout_inner(backend, handle, expected, Some(task)).await
}

async fn wait_for_stdout_inner(
    backend: &crate::execution::docker::backend::DockerExecutionBackend,
    handle: &ExecutionHandleV1,
    expected: &str,
    task: Option<&JoinHandle<Result<(), crate::execution::errors::ExecutionError>>>,
) -> Vec<ExecutionEventV1> {
    let started = tokio::time::Instant::now();
    let mut last_events = Vec::new();
    loop {
        let remaining = STDOUT_WAIT.saturating_sub(started.elapsed());
        let events = match tokio::time::timeout(
            remaining,
            backend.events(handle, EventPageRequestV1::new(0, 256).unwrap()),
        )
        .await
        {
            Ok(Ok(events)) => events,
            Ok(Err(error)) => {
                let record = load_record(handle);
                panic!(
                    "could not read execution events while waiting for stdout {expected:?}: {error}; exec_task_finished={}; {}; observed_events={last_events:?}",
                    task.is_some_and(JoinHandle::is_finished),
                    record_status(record.as_ref()),
                );
            }
            Err(_) => {
                let record = load_record(handle);
                panic!(
                    "timed out waiting for stdout {expected:?} while reading events; exec_task_finished={}; {}; observed_events={last_events:?}",
                    task.is_some_and(JoinHandle::is_finished),
                    record_status(record.as_ref()),
                );
            }
        };
        if events.iter().any(|event| {
            matches!(&event.event, ExecutionEventKindV1::Stdout { chunk } if chunk.contains(expected))
        }) {
            return events;
        }
        last_events.clone_from(&events);
        if task.is_some_and(JoinHandle::is_finished) {
            let remaining = STDOUT_WAIT.saturating_sub(started.elapsed());
            let final_events = match tokio::time::timeout(
                remaining,
                backend.events(handle, EventPageRequestV1::new(0, 256).unwrap()),
            )
            .await
            {
                Ok(Ok(events)) => events,
                Ok(Err(error)) => {
                    let record = load_record(handle);
                    panic!(
                        "exec task finished before expected stdout {expected:?}, then final event read failed: {error}; exec_task_finished=true; {}; observed_events={events:?}",
                        record_status(record.as_ref())
                    );
                }
                Err(_) => {
                    let record = load_record(handle);
                    panic!(
                        "exec task finished before expected stdout {expected:?}, but final event read timed out; exec_task_finished=true; {}; observed_events={events:?}",
                        record_status(record.as_ref())
                    );
                }
            };
            if final_events.iter().any(|event| {
                matches!(&event.event, ExecutionEventKindV1::Stdout { chunk } if chunk.contains(expected))
            }) {
                return final_events;
            }
            let record = load_record(handle);
            panic!(
                "exec task finished before expected stdout {expected:?}: join_handle_finished=true, {}; observed_events={final_events:?}",
                record_status(record.as_ref())
            );
        }
        let record = load_record(handle);
        if record
            .as_ref()
            .is_some_and(|record| is_terminal(record.state))
        {
            panic!(
                "execution ended before expected stdout {expected:?}: {}; observed_events={events:?}",
                record_status(record.as_ref())
            );
        }
        if started.elapsed() >= STDOUT_WAIT {
            let execution_progress = diagnostic_progress(backend, handle);
            panic!(
                "timed out waiting for expected stdout {expected:?}: {}; exec_task_finished={}; {execution_progress}; observed_events={events:?}",
                record_status(record.as_ref()),
                task.is_some_and(JoinHandle::is_finished),
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn load_record(
    handle: &ExecutionHandleV1,
) -> Option<crate::persistence::execution_records::ExecutionRecordV1> {
    crate::persistence::execution_records::load(&handle.task_id, &handle.execution_id)
        .unwrap_or_else(|error| panic!("load execution record while waiting for stdout: {error}"))
}

fn record_status(
    record: Option<&crate::persistence::execution_records::ExecutionRecordV1>,
) -> String {
    let Some(record) = record else {
        return "durable execution record is missing".into();
    };
    let failure = record
        .result
        .as_ref()
        .and_then(|result| result.failure.as_ref())
        .map(ToString::to_string);
    format!(
        "state={:?}, exit_code={:?}, failure={failure:?}, command_id_present={}, output_bytes={}, phase_timings={:?}",
        record.state,
        record.exit_code,
        record.command_id.is_some(),
        record.output_bytes,
        record.phase_timings
    )
}

fn diagnostic_progress(
    backend: &crate::execution::docker::backend::DockerExecutionBackend,
    handle: &ExecutionHandleV1,
) -> String {
    let Some(progress) = backend.diagnostic_progress(handle) else {
        return "exec_progress=unavailable".into();
    };
    format!(
        "exec_progress={{created:{},attached:{},input_written:{},input_closed:{},output_received:{}}}",
        progress & (1 << 0) != 0,
        progress & (1 << 1) != 0,
        progress & (1 << 2) != 0,
        progress & (1 << 3) != 0,
        progress & (1 << 4) != 0,
    )
}

fn is_terminal(state: crate::persistence::execution_records::ExecutionRecordStateV1) -> bool {
    matches!(
        state,
        crate::persistence::execution_records::ExecutionRecordStateV1::Succeeded
            | crate::persistence::execution_records::ExecutionRecordStateV1::Failed
            | crate::persistence::execution_records::ExecutionRecordStateV1::Cancelled
            | crate::persistence::execution_records::ExecutionRecordStateV1::Interrupted
    )
}
