use super::{BridgeRequest, CACHE_HEADER_LIMIT, MESSAGE_LIMIT, SessionBinding, ToolResponse};
use crate::execution::session::ExecutionSession;
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

pub(super) fn serve_request(
    mut stream: UnixStream,
    session: &Arc<ExecutionSession>,
    binding: &SessionBinding,
    cancel: &Arc<AtomicBool>,
    resource_bridge: Option<&crate::harness::resource_bridge::ResourceBridge>,
) -> anyhow::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut line = Vec::new();
    let mut reader = BufReader::new(stream.try_clone()?);
    reader
        .by_ref()
        .take((CACHE_HEADER_LIMIT + 1) as u64)
        .read_until(b'\n', &mut line)?;
    anyhow::ensure!(
        line.len() <= CACHE_HEADER_LIMIT,
        "Execution request header exceeds size limit"
    );
    let request: BridgeRequest = serde_json::from_slice(&line)?;
    let request = match request {
        BridgeRequest::CacheStage(request) => {
            stream.set_read_timeout(Some(Duration::from_secs(1200)))?;
            anyhow::ensure!(
                request.session_id == binding.session_id
                    && request.capability_id == binding.capability_id
                    && request.task_id == binding.task_id
                    && request.workspace_id == binding.workspace_id
                    && request.access == binding.access
                    && request.target_id == binding.target_id
                    && Some(request.target_arch.as_str()) == binding.target_arch.as_deref(),
                "Cache transfer does not match its application-owned session"
            );
            anyhow::ensure!(
                (1..=crate::harness::resource_bridge::MAX_CACHE_SNAPSHOT_BYTES as usize)
                    .contains(&request.archive_bytes),
                "Cache transfer size is outside supported limits"
            );
            let mut archive = vec![0; request.archive_bytes];
            reader.read_exact(&mut archive)?;
            session
                .stage_dependency_cache(crate::harness::resource_bridge::CacheSnapshot {
                    cache_ref: request.cache_ref,
                    manifest: request.manifest,
                    archive,
                })
                .map_err(|error| anyhow::anyhow!(error.message))?;
            serde_json::to_writer(
                &mut stream,
                &ToolResponse {
                    stdout: "Verified dependency cache staged for this execution target.".into(),
                    stderr: String::new(),
                    exit_code: Some(0),
                    is_error: false,
                },
            )?;
            stream.write_all(b"\n")?;
            stream.flush()?;
            return Ok(());
        }
        BridgeRequest::DependencyStart(request) => {
            verify_binding(
                &request.session_id,
                &request.capability_id,
                &request.task_id,
                &request.workspace_id,
                &request.access,
                binding,
            )?;
            anyhow::ensure!(
                request.access == "implementation",
                "dependency relay requires implementation access"
            );
            let prepared = prepare_remote_dependency(session, resource_bridge, request.need)?;
            serde_json::to_writer(&mut stream, &prepared)?;
            stream.write_all(b"\n")?;
            stream.flush()?;
            return Ok(());
        }
        BridgeRequest::Tool(request) => {
            anyhow::ensure!(
                line.len() <= MESSAGE_LIMIT,
                "Execution tool request exceeds size limit"
            );
            request
        }
    };
    verify_binding(
        &request.session_id,
        &request.capability_id,
        &request.task_id,
        &request.workspace_id,
        &request.access,
        binding,
    )
    .map_err(|_| {
        anyhow::anyhow!("Execution tool request does not match its application-owned session")
    })?;
    let timeout = Duration::from_secs(request.timeout_seconds.clamp(1, 1200));
    let done = Arc::new(AtomicBool::new(false));
    let watcher_done = done.clone();
    let cancel_watcher = thread::spawn({
        let session = session.clone();
        let cancel = cancel.clone();
        move || {
            while !watcher_done.load(Ordering::Acquire) {
                if cancel.load(Ordering::Acquire) {
                    let _ = session.cancel();
                    return;
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
    });
    let output = session.run_tool_command(&request.command, timeout);
    done.store(true, Ordering::Release);
    let _ = cancel_watcher.join();
    let response = match output {
        Ok(output) => ToolResponse {
            stdout: output.stdout,
            stderr: output.stderr,
            exit_code: output.result.exit_code,
            is_error: output.result.exit_code != Some(0)
                || output.result.failure.is_some()
                || output.result.timed_out
                || output.result.cancelled,
        },
        Err(error) => ToolResponse {
            stdout: String::new(),
            stderr: error.message,
            exit_code: None,
            is_error: true,
        },
    };
    serde_json::to_writer(&mut stream, &response)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

pub(crate) fn prepare_remote_dependency(
    session: &ExecutionSession,
    resource_bridge: Option<&crate::harness::resource_bridge::ResourceBridge>,
    need: crate::harness::DependencyNeed,
) -> anyhow::Result<super::DependencyRelayStartResponse> {
    let relay = session
        .relay_dependency_request(need)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    let prepared_result = (|| {
        resource_bridge
            .ok_or_else(|| anyhow::anyhow!("Remote resource broker is unavailable"))?
            .bind_remote_execution(
                relay.worker_id.clone(),
                relay.execution_id.clone(),
                relay.task_id.clone(),
                relay.authorization_generation.clone(),
                session.authorization_verifier()?,
            )?
            .prepare(relay.clone())
    })();
    let (outcome, broker_response) = match prepared_result {
        Ok(prepared) => (prepared.relay_response, prepared.broker_response),
        Err(_) => (
            relay_response(
                &relay,
                crate::execution::remote::dependency::DependencyRelayOutcomeV1::Failed,
                None,
            ),
            serde_json::json!({
                "status": "error",
                "summary": "The controller resource broker could not prepare this remote dependency."
            }),
        ),
    };
    session
        .relay_dependency_decision(outcome)
        .map_err(|error| anyhow::anyhow!(error.message))?;
    Ok(super::DependencyRelayStartResponse {
        request: relay,
        broker_response,
    })
}

fn relay_response(
    request: &crate::execution::remote::dependency::DependencyRelayRequestV1,
    outcome: crate::execution::remote::dependency::DependencyRelayOutcomeV1,
    dependency_request_id: Option<String>,
) -> crate::execution::remote::dependency::DependencyRelayResponseV1 {
    crate::execution::remote::dependency::DependencyRelayResponseV1 {
        request_id: request.request_id.clone(),
        worker_id: request.worker_id.clone(),
        execution_id: request.execution_id.clone(),
        task_id: request.task_id.clone(),
        workspace: request.workspace.clone(),
        authorization_generation: request.authorization_generation.clone(),
        cancellation_generation: request.cancellation_generation,
        outcome,
        dependency_request_id,
    }
}

fn verify_binding(
    session_id: &str,
    capability_id: &str,
    task_id: &str,
    workspace_id: &str,
    access: &str,
    binding: &SessionBinding,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        session_id == binding.session_id
            && capability_id == binding.capability_id
            && task_id == binding.task_id
            && workspace_id == binding.workspace_id
            && access == binding.access,
        "Dependency relay does not match its application-owned session"
    );
    Ok(())
}

#[cfg(test)]
#[path = "serve/tests.rs"]
mod tests;
