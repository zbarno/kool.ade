use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc};

use crate::execution::{
    backend::{BackendProbeV1, ExecutionBackend},
    contract::ExecutionTarget,
    errors::ExecutionError,
};

use super::super::protocol::{EnvelopeKindV1 as Kind, EnvelopeV1};

mod cache_transfer;
mod support;
pub(super) use cache_transfer::CacheTransfers;
pub(super) type PendingDependencyRelays =
    HashMap<String, crate::execution::remote::dependency::DependencyRelayRequestV1>;
use support::{decode, json_value, require_control_id, require_execution_id};

mod requests;
use requests::*;

pub(super) struct DispatchContext<'a> {
    pub backend: Arc<dyn ExecutionBackend>,
    pub stager: Option<Arc<crate::execution::remote::staging::WorkerStager>>,
    pub cache_transfers: &'a mut CacheTransfers,
    pub pending_dependency_relays: &'a mut PendingDependencyRelays,
    pub capacity: Option<&'a super::super::WorkerCapacityPool>,
    pub capacity_leases: &'a mut HashMap<String, super::super::WorkerCapacityLease>,
    pub runtime: &'a tokio::runtime::Runtime,
}

pub(super) fn dispatch(
    context: DispatchContext<'_>,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), ExecutionError> {
    let DispatchContext {
        backend,
        stager,
        cache_transfers,
        pending_dependency_relays,
        capacity,
        capacity_leases,
        runtime,
    } = context;
    let result = match envelope.kind {
        Kind::DependencyRequest => Err(super::incompatible(
            "worker dependency requests are emitted only as relay responses",
        )),
        Kind::Probe => {
            require_control_id(envelope)?;
            let request: ProbeRequest = decode(&envelope.payload)?;
            let mut probe = runtime.block_on(backend.probe(&request.target))?;
            if let (Some(capabilities), Some(pool)) = (&mut probe.capabilities, capacity) {
                capabilities.maximum_execution_count = Some(pool.max_jobs());
            }
            json_value(probe).map(|value| (Kind::ProbeResult, value))
        }
        Kind::PrepareWorkspace => {
            require_control_id(envelope)?;
            let request: PrepareRequest = decode(&envelope.payload)?;
            json_value(
                runtime.block_on(backend.prepare_workspace(&request.workspace, &request.target))?,
            )
            .map(|value| (Kind::Acknowledge, value))
        }
        Kind::BeginWorkspaceTransfer => {
            require_control_id(envelope)?;
            let request: BeginWorkspaceTransferRequest = decode(&envelope.payload)?;
            stager
                .as_ref()
                .ok_or_else(|| super::incompatible("worker workspace staging is unavailable"))?
                .begin(request.manifest)
                .map_err(|_| super::incompatible("workspace transfer manifest was rejected"))?;
            Ok((Kind::Acknowledge, json!({"accepted": true})))
        }
        Kind::WorkspaceTransferChunk => {
            require_control_id(envelope)?;
            let request: WorkspaceTransferChunkRequest = decode(&envelope.payload)?;
            stager
                .as_ref()
                .ok_or_else(|| super::incompatible("worker workspace staging is unavailable"))?
                .accept_chunk(&request.chunk)
                .map_err(|_| super::incompatible("workspace transfer chunk was rejected"))?;
            Ok((Kind::Acknowledge, json!({"accepted": true})))
        }
        Kind::FinalizeWorkspaceTransfer => {
            require_control_id(envelope)?;
            let request: FinalizeWorkspaceTransferRequest = decode(&envelope.payload)?;
            stager
                .as_ref()
                .ok_or_else(|| super::incompatible("worker workspace staging is unavailable"))?
                .finalize(&request.transfer_id)
                .map_err(|_| super::incompatible("workspace transfer could not be finalized"))?;
            Ok((Kind::Acknowledge, json!({"accepted": true})))
        }
        Kind::BeginCacheTransfer => {
            cache_transfer::begin(&envelope.payload, cache_transfers, envelope)
        }
        Kind::CacheTransferChunk => {
            cache_transfer::chunk(&envelope.payload, cache_transfers, envelope)
        }
        Kind::FinalizeCacheTransfer => cache_transfer::finalize(
            &envelope.payload,
            cache_transfers,
            envelope,
            backend,
            runtime,
        ),
        Kind::RelayDependency => {
            let request: crate::execution::remote::dependency::DependencyRelayRequestV1 =
                decode(&envelope.payload)?;
            request
                .validate()
                .map_err(|_| super::incompatible("dependency request is invalid"))?;
            require_execution_id(envelope, &request.execution_id)?;
            let stager = stager
                .as_ref()
                .ok_or_else(|| super::incompatible("worker dependency relay is unavailable"))?;
            if request.worker_id != stager.worker_id()
                || !stager.has_workspace(&request.workspace)
                || pending_dependency_relays.contains_key(&request.request_id)
            {
                return Err(super::incompatible(
                    "dependency request is outside the staged worker workspace",
                ));
            }
            pending_dependency_relays.insert(request.request_id.clone(), request.clone());
            json_value(request).map(|value| (Kind::DependencyRequest, value))
        }
        Kind::DependencyDecision => {
            let response: crate::execution::remote::dependency::DependencyRelayResponseV1 =
                decode(&envelope.payload)?;
            require_execution_id(envelope, &response.execution_id)?;
            let request = pending_dependency_relays
                .get(&response.request_id)
                .cloned()
                .ok_or_else(|| {
                    super::incompatible("dependency decision has no pending worker request")
                })?;
            if !request.matches_response(&response)
                || (response.outcome
                    == crate::execution::remote::dependency::DependencyRelayOutcomeV1::Prepared
                    && response.dependency_request_id.as_deref().is_none_or(|id| {
                        crate::execution::contract::validate_identifier(id).is_err()
                    }))
                || (response.outcome
                    != crate::execution::remote::dependency::DependencyRelayOutcomeV1::Prepared
                    && response.dependency_request_id.is_some())
            {
                return Err(super::incompatible(
                    "dependency decision does not match its pending request",
                ));
            }
            pending_dependency_relays.remove(&response.request_id);
            Ok((Kind::Acknowledge, json!({"accepted": true})))
        }
        Kind::PrepareWorkspaceResult => {
            let request: PrepareWorkspaceResultRequest = decode(&envelope.payload)?;
            require_execution_id(envelope, &request.handle.execution_id)?;
            if request.workspace.task_id != request.handle.task_id {
                return Err(super::incompatible(
                    "workspace result task does not match handle",
                ));
            }
            runtime.block_on(backend.collect_artifacts(&request.handle))?;
            let manifest = stager
                .as_ref()
                .ok_or_else(|| super::incompatible("worker workspace staging is unavailable"))?
                .create_workspace_result(
                    &request.handle.execution_id,
                    &request.workspace,
                    &request.snapshot_generation,
                    &request.destination_identity,
                );
            match manifest {
                Ok(manifest) => json_value(manifest).map(|value| (Kind::Acknowledge, value)),
                Err(_) => Err(super::incompatible(
                    "verified workspace result could not be prepared",
                )),
            }
        }
        Kind::ReadWorkspaceResultChunk => {
            let request: ReadWorkspaceResultChunkRequest = decode(&envelope.payload)?;
            require_execution_id(envelope, &request.handle.execution_id)?;
            if request.workspace.task_id != request.handle.task_id {
                return Err(super::incompatible(
                    "workspace result task does not match handle",
                ));
            }
            let chunk = stager
                .as_ref()
                .ok_or_else(|| super::incompatible("worker workspace staging is unavailable"))?
                .read_result_chunk(
                    &request.handle.execution_id,
                    &request.transfer_id,
                    &request.workspace,
                    &request.path,
                    request.offset,
                )
                .map_err(|_| super::incompatible("workspace result chunk is unavailable"))?;
            json_value(chunk).map(|value| (Kind::Acknowledge, value))
        }
        Kind::Start => {
            let request: StartRequest = decode(&envelope.payload)?;
            require_execution_id(envelope, &request.request.execution_id)?;
            if capacity_leases.contains_key(&request.request.execution_id) {
                return Err(super::incompatible(
                    "worker execution capacity is already reserved",
                ));
            }
            let reservation = capacity
                .map(|pool| {
                    pool.acquire().map_err(|_| {
                        super::incompatible("remote worker execution capacity is full")
                    })
                })
                .transpose()?;
            let handle = runtime.block_on(backend.start(&request.lease, &request.request))?;
            if let Some(reservation) = reservation {
                capacity_leases.insert(request.request.execution_id.clone(), reservation);
            }
            json_value(handle).map(|value| (Kind::Acknowledge, value))
        }
        Kind::Exec => {
            let request: ExecRequest = decode(&envelope.payload)?;
            require_execution_id(envelope, &request.handle.execution_id)?;
            let backend = backend.clone();
            runtime.spawn(async move {
                let _ = backend.exec(&request.handle, &request.request).await;
            });
            Ok((Kind::Acknowledge, json!({"accepted": true})))
        }
        Kind::Subscribe => {
            let request: EventsRequest = decode(&envelope.payload)?;
            require_execution_id(envelope, &request.handle.execution_id)?;
            json_value(runtime.block_on(backend.events(&request.handle, request.page))?)
                .map(|value| (Kind::Event, value))
        }
        Kind::Cancel => {
            let request: CancelRequest = decode(&envelope.payload)?;
            require_execution_id(envelope, &request.handle.execution_id)?;
            runtime.block_on(backend.cancel(&request.handle, request.cancellation_generation))?;
            capacity_leases.remove(&request.handle.execution_id);
            Ok((Kind::Acknowledge, json!({"accepted": true})))
        }
        Kind::CollectArtifacts => {
            let request: HandleRequest = decode(&envelope.payload)?;
            require_execution_id(envelope, &request.handle.execution_id)?;
            json_value(runtime.block_on(backend.collect_artifacts(&request.handle))?)
                .map(|value| (Kind::Acknowledge, value))
        }
        Kind::Release => {
            let request: HandleRequest = decode(&envelope.payload)?;
            require_execution_id(envelope, &request.handle.execution_id)?;
            runtime.block_on(backend.release(&request.handle))?;
            capacity_leases.remove(&request.handle.execution_id);
            Ok((Kind::Acknowledge, json!({"released": true})))
        }
        Kind::ReleaseWorkspace => {
            require_control_id(envelope)?;
            let request: ReleaseWorkspaceRequest = decode(&envelope.payload)?;
            runtime.block_on(backend.release_workspace(&request.lease))?;
            Ok((Kind::Acknowledge, json!({"released": true})))
        }
        Kind::Health => {
            require_control_id(envelope)?;
            let request: HealthRequest = decode(&envelope.payload)?;
            if request.wire_version != super::super::protocol::PROTOCOL_VERSION_V1 {
                return Err(super::incompatible(
                    "remote worker protocol version is unsupported",
                ));
            }
            let target = ExecutionTarget::local();
            let mut probe: BackendProbeV1 = runtime.block_on(backend.probe(&target))?;
            if let (Some(capabilities), Some(pool)) = (&mut probe.capabilities, capacity) {
                capabilities.maximum_execution_count = Some(pool.max_jobs());
            }
            Ok((
                Kind::Acknowledge,
                json!({"wire_version": request.wire_version, "probe": probe}),
            ))
        }
        Kind::Heartbeat => {
            require_control_id(envelope)?;
            let request: HeartbeatRequest = decode(&envelope.payload)?;
            if request.nonce.is_empty() || request.nonce.len() > 128 {
                return Err(super::incompatible("remote worker heartbeat is invalid"));
            }
            json_value(request).map(|value| (Kind::Acknowledge, value))
        }
        Kind::ProbeResult | Kind::Event | Kind::Acknowledge | Kind::Error => {
            return Err(super::incompatible(
                "remote worker received a response as a request",
            ));
        }
    };
    result.map_err(|_| super::incompatible("remote worker response could not be encoded"))
}
