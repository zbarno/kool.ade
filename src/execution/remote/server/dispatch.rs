use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::execution::{backend::ExecutionBackend, errors::ExecutionError};

use super::super::protocol::{EnvelopeKindV1 as Kind, EnvelopeV1};
mod cache_transfer;
mod exec;
mod health;
mod private_files;
mod release;
mod support;
mod system_image;
mod workspace;
pub(super) use cache_transfer::CacheTransfers;
use support::{
    PendingDependencyRelays, decode, json_value, require_control_id, require_execution_id,
};
pub(super) use system_image::SystemImageTransfers;

mod requests;
mod start;
use requests::*;

pub(super) struct DispatchContext<'a> {
    pub backend: Arc<dyn ExecutionBackend>,
    pub stager: Option<Arc<crate::execution::remote::staging::WorkerStager>>,
    pub cache_transfers: &'a mut CacheTransfers,
    pub system_image_transfers: &'a mut SystemImageTransfers,
    pub pending_dependency_relays: &'a mut PendingDependencyRelays,
    pub capacity: Option<&'a super::super::WorkerCapacityPool>,
    pub capacity_leases: Arc<Mutex<HashMap<String, super::super::WorkerCapacityLease>>>,
    pub runtime: &'a tokio::runtime::Runtime,
    pub system_image_capability_key: &'a mut Option<zeroize::Zeroizing<[u8; 32]>>,
}

pub(super) fn dispatch(
    context: DispatchContext<'_>,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), ExecutionError> {
    let DispatchContext {
        backend,
        stager,
        cache_transfers,
        system_image_transfers,
        pending_dependency_relays,
        capacity,
        capacity_leases,
        runtime,
        system_image_capability_key,
    } = context;
    if matches!(
        envelope.kind,
        Kind::PrepareWorkspace
            | Kind::BeginWorkspaceTransfer
            | Kind::WorkspaceTransferChunk
            | Kind::FinalizeWorkspaceTransfer
            | Kind::ReleaseWorkspace
    ) {
        return workspace::dispatch(backend, stager, runtime, envelope);
    }
    if matches!(
        envelope.kind,
        Kind::InspectSystemPackages
            | Kind::BeginSystemImageTransfer
            | Kind::SystemImageTransferChunk
            | Kind::FinalizeSystemImageTransfer
    ) {
        return system_image::dispatch(
            backend,
            stager.as_ref(),
            system_image_transfers,
            runtime,
            system_image_capability_key,
            envelope,
        );
    }
    if matches!(
        envelope.kind,
        Kind::AuthorizePrivateFileGrant | Kind::AbortPrivateFileStage
    ) {
        return private_files::dispatch(stager.as_ref(), runtime, envelope);
    }
    match envelope.kind {
        Kind::PrivateFileChunk => Err(super::incompatible(
            "private-file chunks require the dedicated binary transport",
        )),
        Kind::DependencyRequest => Err(super::incompatible(
            "worker dependency requests are emitted only as relay responses",
        )),
        Kind::Probe => {
            require_control_id(envelope)?;
            let request: ProbeRequest = decode(&envelope.payload)?;
            let mut probe = runtime.block_on(backend.probe(&request.target))?;
            if let Some(stager) = &stager {
                stager
                    .cleanup_recovered_private_file_executions()
                    .map_err(|_| {
                        super::incompatible("recovered private files could not be cleaned")
                    })?;
            }
            if let (Some(capabilities), Some(pool)) = (&mut probe.capabilities, capacity) {
                capabilities.maximum_execution_count = Some(pool.max_jobs());
                let reserved = pool.reserved_resources().ok();
                capabilities.available_cpu_millis =
                    crate::execution::resources::effective_cpu_capacity_millis()
                        .and_then(|capacity| capacity.checked_sub(reserved?.cpu_millis));
                capabilities.available_memory_bytes =
                    crate::execution::resources::effective_memory_capacity_bytes()
                        .and_then(|capacity| capacity.checked_sub(reserved?.memory_bytes));
            }
            json_value(probe).map(|value| (Kind::ProbeResult, value))
        }
        Kind::BeginCacheTransfer | Kind::CacheTransferChunk | Kind::FinalizeCacheTransfer => {
            cache_transfer::dispatch(
                &envelope.payload,
                cache_transfers,
                envelope,
                backend,
                runtime,
            )
        }
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
        Kind::Start => start::dispatch(
            backend,
            stager,
            capacity,
            capacity_leases,
            runtime,
            envelope,
        ),
        Kind::Exec => exec::dispatch(backend, stager, runtime, envelope),
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
            capacity_leases
                .lock()
                .map_err(|_| super::incompatible("worker capacity registry is unavailable"))?
                .remove(&request.handle.execution_id);
            if let Some(stager) = &stager {
                stager
                    .cleanup_stopped_private_file_execution(&request.handle.execution_id)
                    .map_err(|_| super::incompatible("private-file stage could not be stopped"))?;
            }
            Ok((Kind::Acknowledge, json!({"accepted": true})))
        }
        Kind::CollectArtifacts => {
            let request: HandleRequest = decode(&envelope.payload)?;
            require_execution_id(envelope, &request.handle.execution_id)?;
            json_value(runtime.block_on(backend.collect_artifacts(&request.handle))?)
                .map(|value| (Kind::Acknowledge, value))
        }
        Kind::Release => release::dispatch(backend, stager, capacity_leases, runtime, envelope),

        Kind::Health => health::dispatch(
            backend,
            stager.as_deref(),
            runtime,
            capacity,
            system_image_capability_key,
            envelope,
        ),
        Kind::Heartbeat => {
            require_control_id(envelope)?;
            let request: HeartbeatRequest = decode(&envelope.payload)?;
            if request.nonce.is_empty() || request.nonce.len() > 128 {
                return Err(super::incompatible("remote worker heartbeat is invalid"));
            }
            json_value(request).map(|value| (Kind::Acknowledge, value))
        }
        Kind::PrepareWorkspace
        | Kind::BeginWorkspaceTransfer
        | Kind::WorkspaceTransferChunk
        | Kind::FinalizeWorkspaceTransfer
        | Kind::ReleaseWorkspace => unreachable!(),
        Kind::ProbeResult | Kind::Event | Kind::Acknowledge | Kind::Error => Err(
            super::incompatible("remote worker received a response as a request"),
        ),
        _ => unreachable!("routed message kinds are handled before general dispatch"),
    }
}
