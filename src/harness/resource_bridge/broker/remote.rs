use super::ResourceBridge;
use crate::execution::remote::dependency::*;
use std::{
    io::{self, Read, Write},
    os::unix::net::UnixStream,
    sync::Arc,
    time::{Duration, Instant},
};

const RESPONSE_POLL: Duration = Duration::from_secs(1);
// Leave room for the validated relay request in the app/MCP session envelope.
const MAX_RESPONSE_BYTES: usize = 47 * 1024;
const MAX_BROKER_WAIT: Duration = Duration::from_secs(20 * 60);

pub(crate) struct RemoteDependencyPreparation {
    pub relay_response: DependencyRelayResponseV1,
    pub broker_response: serde_json::Value,
}

/// Capability to request dependency preparation for one authenticated active
/// worker execution. It cannot approve requests or access cache bytes.
#[derive(Clone)]
pub(crate) struct RemoteDependencyBridge {
    socket: std::path::PathBuf,
    worker_id: String,
    execution_id: String,
    task_id: String,
    authorization_generation: String,
    verifier: Arc<dyn crate::execution::session::AuthorizationGenerationVerifier>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl ResourceBridge {
    pub(crate) fn bind_remote_execution(
        &self,
        worker_id: String,
        execution_id: String,
        task_id: String,
        authorization_generation: String,
        verifier: Arc<dyn crate::execution::session::AuthorizationGenerationVerifier>,
    ) -> anyhow::Result<RemoteDependencyBridge> {
        anyhow::ensure!(
            broker_task_matches(self.task_id.as_deref(), &task_id),
            "remote dependency binding does not match the broker task"
        );
        crate::execution::contract::validate_identifier(&worker_id)?;
        crate::execution::contract::validate_identifier(&execution_id)?;
        crate::execution::contract::validate_identifier(&task_id)?;
        anyhow::ensure!(
            verifier.generation() == authorization_generation,
            "remote dependency binding authorization generation is stale"
        );
        verifier
            .verify(&authorization_generation)
            .map_err(|_| anyhow::anyhow!("remote dependency binding is no longer authorized"))?;
        Ok(RemoteDependencyBridge {
            socket: self.socket.clone(),
            worker_id,
            execution_id,
            task_id,
            authorization_generation,
            verifier,
            cancel: self.cancel.clone(),
            stop: self.stop.clone(),
        })
    }
}

impl RemoteDependencyBridge {
    /// Routes metadata through the ordinary broker socket, preserving the same
    /// triage, approval wait, cancellation, and package adapter path as local
    /// resource requests.
    pub(crate) fn prepare(
        &self,
        request: DependencyRelayRequestV1,
    ) -> anyhow::Result<RemoteDependencyPreparation> {
        self.verify_active()?;
        request.validate()?;
        anyhow::ensure!(
            binding_matches(
                &request,
                &self.worker_id,
                &self.execution_id,
                &self.task_id,
                &self.authorization_generation,
            ),
            "dependency relay identity does not match the active execution"
        );
        let mut stream = UnixStream::connect(&self.socket)?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        stream.set_read_timeout(Some(RESPONSE_POLL))?;
        let deadline = Instant::now() + MAX_BROKER_WAIT;
        let resource_request = super::super::ResourceRequest {
            action: super::super::ResourceAction::DependencyRequest,
            manager: None,
            url: None,
            dependency: Some(request.need.clone()),
            dependency_request_id: None,
            retry_succeeded: None,
            purpose: request.need.reason.clone(),
        };
        let mut bytes = serde_json::to_vec(&resource_request)?;
        anyhow::ensure!(bytes.len() <= super::super::MAX_REQUEST_BYTES);
        bytes.push(b'\n');
        stream.write_all(&bytes)?;
        let response = read_bounded_response_until(&mut stream, self, deadline)?;
        self.verify_active()?;
        let outcome = relay_outcome(&response.status);
        let dependency_request_id = if outcome == DependencyRelayOutcomeV1::Prepared {
            response
                .dependency_request
                .as_ref()
                .map(|request| request.id.clone())
                .filter(|id| crate::execution::contract::validate_identifier(id).is_ok())
        } else {
            None
        };
        let outcome =
            if outcome == DependencyRelayOutcomeV1::Prepared && dependency_request_id.is_none() {
                DependencyRelayOutcomeV1::Failed
            } else {
                outcome
            };
        let relay_response = DependencyRelayResponseV1 {
            request_id: request.request_id.clone(),
            worker_id: request.worker_id.clone(),
            execution_id: request.execution_id.clone(),
            task_id: request.task_id.clone(),
            workspace: request.workspace.clone(),
            authorization_generation: request.authorization_generation.clone(),
            cancellation_generation: request.cancellation_generation,
            outcome,
            dependency_request_id,
        };
        anyhow::ensure!(request.matches_response(&relay_response));
        Ok(RemoteDependencyPreparation {
            relay_response,
            broker_response: serde_json::to_value(response)?,
        })
    }

    fn verify_active(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.cancel.load(std::sync::atomic::Ordering::Acquire)
                && !self.stop.load(std::sync::atomic::Ordering::Acquire),
            "remote dependency request was cancelled"
        );
        self.verifier
            .verify(&self.authorization_generation)
            .map_err(|_| anyhow::anyhow!("remote dependency authorization is stale"))
    }
}

fn broker_task_matches(broker_task: Option<&str>, request_task: &str) -> bool {
    broker_task == Some(request_task)
}

fn binding_matches(
    request: &DependencyRelayRequestV1,
    worker_id: &str,
    execution_id: &str,
    task_id: &str,
    authorization_generation: &str,
) -> bool {
    request.worker_id == worker_id
        && request.execution_id == execution_id
        && request.task_id == task_id
        && request.authorization_generation == authorization_generation
}

fn read_bounded_response_until(
    stream: &mut UnixStream,
    binding: &RemoteDependencyBridge,
    deadline: Instant,
) -> anyhow::Result<super::super::ResourceResponse> {
    let mut bytes = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => anyhow::bail!("resource broker closed the response"),
            Ok(_) if byte[0] == b'\n' => break,
            Ok(_) => {
                anyhow::ensure!(
                    bytes.len() < MAX_RESPONSE_BYTES,
                    "resource broker response is too large"
                );
                bytes.push(byte[0]);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "resource broker response timed out"
                );
                binding.verify_active()?;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn relay_outcome(status: &str) -> DependencyRelayOutcomeV1 {
    match status {
        "prepared" | "allowed" => DependencyRelayOutcomeV1::Prepared,
        "needs_attention" => DependencyRelayOutcomeV1::AwaitingManager,
        "rejected" | "denied" => DependencyRelayOutcomeV1::Denied,
        _ => DependencyRelayOutcomeV1::Failed,
    }
}

#[cfg(test)]
#[path = "remote/tests.rs"]
mod tests;
