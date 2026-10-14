use super::{EnvelopeV1, ExecutionBackend, Kind, Value};
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashMap, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeginRequest {
    transfer_id: String,
    cache_ref: crate::execution::contract::DependencyCacheRefV1,
    manifest: crate::harness::resource_bridge::CacheSnapshotManifest,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChunkRequest {
    chunk: crate::execution::remote::staging::TransferChunkV1,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FinalizeRequest {
    transfer_id: String,
}

#[derive(Default)]
pub(crate) struct CacheTransfers(HashMap<String, Pending>);

struct Pending {
    cache_ref: crate::execution::contract::DependencyCacheRefV1,
    manifest: crate::harness::resource_bridge::CacheSnapshotManifest,
    archive: Vec<u8>,
}

pub(super) fn begin(
    payload: &Value,
    transfers: &mut CacheTransfers,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), crate::execution::errors::ExecutionError> {
    super::support::require_control_id(envelope)?;
    let request: BeginRequest = super::support::decode(payload)?;
    let id = uuid::Uuid::parse_str(&request.transfer_id)
        .map_err(|_| super::super::incompatible("cache transfer ID is invalid"))?;
    if id.hyphenated().to_string() != request.transfer_id {
        return Err(super::super::incompatible("cache transfer ID is invalid"));
    }
    request
        .cache_ref
        .validate()
        .map_err(|_| super::super::incompatible("cache reference was rejected"))?;
    if request.cache_ref.size_bytes > crate::harness::resource_bridge::MAX_CACHE_SNAPSHOT_BYTES
        || !transfers.0.is_empty()
        || transfers.0.contains_key(&request.transfer_id)
    {
        return Err(super::super::incompatible(
            "cache transfer capacity or size was rejected",
        ));
    }
    let manifest = &request.manifest;
    if manifest.entries.is_empty()
        || manifest.entries.len() > 4_096
        || manifest.task_id != request.cache_ref.task_id
        || manifest.project_id != request.cache_ref.project_id
        || manifest.ecosystem != request.cache_ref.ecosystem
        || manifest.target_id != request.cache_ref.target_id
        || manifest.target_arch != request.cache_ref.target_arch
        || manifest.resolved_integrity != request.cache_ref.resolved_integrity
    {
        return Err(super::super::incompatible(
            "cache manifest scope was rejected",
        ));
    }
    transfers.0.insert(
        request.transfer_id,
        Pending {
            archive: Vec::with_capacity(request.cache_ref.size_bytes as usize),
            cache_ref: request.cache_ref,
            manifest: request.manifest,
        },
    );
    Ok((Kind::Acknowledge, json!({"accepted": true})))
}

pub(super) fn chunk(
    payload: &Value,
    transfers: &mut CacheTransfers,
    envelope: &EnvelopeV1,
) -> Result<(Kind, Value), crate::execution::errors::ExecutionError> {
    super::support::require_control_id(envelope)?;
    let request: ChunkRequest = super::support::decode(payload)?;
    request
        .chunk
        .validate()
        .map_err(|_| super::super::incompatible("cache chunk was rejected"))?;
    if request.chunk.path != "cache.snapshot" {
        return Err(super::super::incompatible("cache chunk path was rejected"));
    }
    let transfer = transfers
        .0
        .get_mut(&request.chunk.transfer_id)
        .ok_or_else(|| super::super::incompatible("cache transfer is unavailable"))?;
    if request.chunk.offset != transfer.archive.len() as u64
        || transfer
            .archive
            .len()
            .saturating_add(request.chunk.bytes.len())
            > transfer.cache_ref.size_bytes as usize
    {
        return Err(super::super::incompatible(
            "cache chunk sequence or size was rejected",
        ));
    }
    transfer.archive.extend_from_slice(&request.chunk.bytes);
    Ok((Kind::Acknowledge, json!({"accepted": true})))
}

pub(super) fn finalize(
    payload: &Value,
    transfers: &mut CacheTransfers,
    envelope: &EnvelopeV1,
    backend: Arc<dyn ExecutionBackend>,
    runtime: &tokio::runtime::Runtime,
) -> Result<(Kind, Value), crate::execution::errors::ExecutionError> {
    super::support::require_control_id(envelope)?;
    let request: FinalizeRequest = super::support::decode(payload)?;
    let transfer = transfers
        .0
        .remove(&request.transfer_id)
        .ok_or_else(|| super::super::incompatible("cache transfer is unavailable"))?;
    if transfer.archive.len() as u64 != transfer.cache_ref.size_bytes {
        return Err(super::super::incompatible("cache transfer is incomplete"));
    }
    transfer
        .manifest
        .validate_ref(&transfer.cache_ref, &transfer.archive)
        .map_err(|_| super::super::incompatible("cache snapshot integrity validation failed"))?;
    runtime
        .block_on(backend.stage_dependency_cache(
            crate::execution::backend::DependencyCacheSnapshot {
                snapshot: crate::harness::resource_bridge::CacheSnapshot {
                    cache_ref: transfer.cache_ref,
                    manifest: transfer.manifest,
                    archive: transfer.archive,
                },
            },
        ))
        .map_err(|error| {
            crate::execution::errors::ExecutionError::new(
                error.kind,
                format!(
                    "Worker backend could not stage the authorized dependency cache: {}",
                    error.message
                ),
                error.retryable,
            )
        })?;
    Ok((Kind::Acknowledge, json!({"accepted": true})))
}
