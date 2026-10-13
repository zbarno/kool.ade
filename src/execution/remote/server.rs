//! Stdio RPC dispatcher used by an SSH-authenticated remote worker process.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::Arc;

use crate::execution::{backend::ExecutionBackend, errors::ExecutionError};

mod dispatch;
mod journal;

#[cfg(test)]
#[path = "server/tests.rs"]
mod tests;

use super::protocol::{EnvelopeKindV1, EnvelopeV1, SessionValidator, read_frame, write_frame};
use super::staging::WorkerStager;
use dispatch::{CacheTransfers, DispatchContext, dispatch};
use journal::{Lookup, RequestJournal};

/// Serve one bounded framed session. SSH performs peer authentication; this
/// layer validates protocol identity and delegates all authority checks to the
/// worker's application-owned `ExecutionBackend`.
pub fn serve_stdio(
    backend: Arc<dyn ExecutionBackend>,
    input: impl Read,
    output: impl Write,
) -> io::Result<()> {
    serve_stdio_with_stager(backend, None, input, output)
}

pub fn serve_stdio_with_stager(
    backend: Arc<dyn ExecutionBackend>,
    stager: Option<Arc<WorkerStager>>,
    input: impl Read,
    output: impl Write,
) -> io::Result<()> {
    serve_stdio_with_capacity(backend, stager, None, input, output)
}

pub(crate) fn serve_stdio_with_capacity(
    backend: Arc<dyn ExecutionBackend>,
    stager: Option<Arc<WorkerStager>>,
    capacity: Option<Arc<super::WorkerCapacityPool>>,
    input: impl Read,
    output: impl Write,
) -> io::Result<()> {
    serve_stdio_with_journal(
        backend,
        stager,
        capacity,
        input,
        output,
        RequestJournal::new(crate::persistence::state_root().join("remote-rpc-journal")),
    )
}

fn serve_stdio_with_journal(
    backend: Arc<dyn ExecutionBackend>,
    stager: Option<Arc<WorkerStager>>,
    capacity: Option<Arc<super::WorkerCapacityPool>>,
    mut input: impl Read,
    mut output: impl Write,
    journal: RequestJournal,
) -> io::Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let mut inbound = SessionValidator::default();
    let mut outbound = SessionValidator::default();
    let mut cache_transfers = CacheTransfers::default();
    let mut pending_dependency_relays = HashMap::new();
    let mut capacity_leases = HashMap::<String, super::WorkerCapacityLease>::new();
    while let Some(request) = read_frame(&mut input)? {
        inbound.accept(&request)?;
        match journal.reserve(&request)? {
            Lookup::Replay(response) => {
                if request.kind == EnvelopeKindV1::RelayDependency {
                    let relay: crate::execution::remote::dependency::DependencyRelayRequestV1 =
                        serde_json::from_value(request.payload.clone()).map_err(|_| {
                            io::Error::other("dependency relay journal request is invalid")
                        })?;
                    relay.validate().map_err(|_| {
                        io::Error::other("dependency relay journal identity is invalid")
                    })?;
                    let staged = stager.as_ref().is_some_and(|worker| {
                        worker.worker_id() == relay.worker_id
                            && worker.has_workspace(&relay.workspace)
                    });
                    if !staged || relay.execution_id != request.execution_id {
                        return Err(io::Error::other(
                            "dependency relay replay is outside the staged workspace",
                        ));
                    }
                    pending_dependency_relays.insert(relay.request_id.clone(), relay);
                }
                outbound.accept(&response)?;
                write_frame(&mut output, &response)?;
                continue;
            }
            Lookup::Pending => {
                let response = EnvelopeV1 {
                    version: super::protocol::PROTOCOL_VERSION_V1,
                    request_id: request.request_id.clone(),
                    execution_id: request.execution_id.clone(),
                    sequence: request.sequence,
                    kind: EnvelopeKindV1::Error,
                    payload: serde_json::to_value(ExecutionError::new(
                        crate::execution::errors::ExecutionErrorKind::WorkerDisconnected,
                        "A prior request has an uncertain outcome and will not be rerun",
                        true,
                    ))
                    .unwrap_or_default(),
                };
                outbound.accept(&response)?;
                write_frame(&mut output, &response)?;
                continue;
            }
            Lookup::Expired => {
                let response = EnvelopeV1 {
                    version: super::protocol::PROTOCOL_VERSION_V1,
                    request_id: request.request_id.clone(),
                    execution_id: request.execution_id.clone(),
                    sequence: request.sequence,
                    kind: EnvelopeKindV1::Error,
                    payload: serde_json::to_value(ExecutionError::new(
                        crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
                        "Remote request replay retention has expired; it will not be rerun",
                        false,
                    ))
                    .unwrap_or_default(),
                };
                outbound.accept(&response)?;
                write_frame(&mut output, &response)?;
                continue;
            }
            Lookup::New => {}
        }
        let (kind, payload) = match dispatch(
            DispatchContext {
                backend: backend.clone(),
                stager: stager.clone(),
                cache_transfers: &mut cache_transfers,
                pending_dependency_relays: &mut pending_dependency_relays,
                capacity: capacity.as_deref(),
                capacity_leases: &mut capacity_leases,
                runtime: &runtime,
            },
            &request,
        ) {
            Ok(response) => response,
            Err(error) => (
                EnvelopeKindV1::Error,
                serde_json::to_value(error).unwrap_or_default(),
            ),
        };
        let response = EnvelopeV1 {
            version: super::protocol::PROTOCOL_VERSION_V1,
            request_id: request.request_id.clone(),
            execution_id: request.execution_id.clone(),
            sequence: request.sequence,
            kind,
            payload,
        };
        journal.complete(&request, &response)?;
        outbound.accept(&response)?;
        write_frame(&mut output, &response)?;
    }
    Ok(())
}

pub(super) fn incompatible(message: &'static str) -> ExecutionError {
    ExecutionError::new(
        crate::execution::errors::ExecutionErrorKind::WorkerIncompatible,
        message,
        false,
    )
}
