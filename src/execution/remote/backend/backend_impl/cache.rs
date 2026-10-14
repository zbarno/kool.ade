use super::*;

pub(super) fn stage<'a>(
    backend: &'a RemoteExecutionBackend,
    snapshot: crate::execution::backend::DependencyCacheSnapshot,
) -> BackendFuture<'a, ()> {
    Box::pin(async move {
        let snapshot = snapshot.snapshot;
        snapshot
            .manifest
            .validate_ref(&snapshot.cache_ref, &snapshot.archive)
            .map_err(|_| incompatible())?;
        if snapshot.cache_ref.target_id != backend.target.target_id {
            return Err(incompatible());
        }
        let transfer_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let mut client = backend.client.lock().map_err(|_| disconnected())?;
        let _: crate::execution::remote::protocol::EnvelopeV1 = client.request(
                EnvelopeKindV1::BeginCacheTransfer,
                CONTROL_EXECUTION_ID,
                serde_json::json!({"transfer_id": transfer_id, "cache_ref": snapshot.cache_ref, "manifest": snapshot.manifest}),
            ).map_err(|error| transfer_error("begin", error))?;
        for (index, bytes) in snapshot
            .archive
            .chunks(crate::execution::remote::staging::MAX_TRANSFER_CHUNK_BYTES)
            .enumerate()
        {
            let chunk = crate::execution::remote::staging::TransferChunkV1::new(
                &transfer_id,
                "cache.snapshot",
                (index * crate::execution::remote::staging::MAX_TRANSFER_CHUNK_BYTES) as u64,
                bytes.to_vec(),
            )
            .map_err(|_| incompatible())?;
            let _: crate::execution::remote::protocol::EnvelopeV1 = client
                .request(
                    EnvelopeKindV1::CacheTransferChunk,
                    CONTROL_EXECUTION_ID,
                    serde_json::json!({"chunk": chunk}),
                )
                .map_err(|error| transfer_error("send chunk", error))?;
        }
        let _: crate::execution::remote::protocol::EnvelopeV1 = client
            .request(
                EnvelopeKindV1::FinalizeCacheTransfer,
                CONTROL_EXECUTION_ID,
                serde_json::json!({"transfer_id": transfer_id}),
            )
            .map_err(|error| transfer_error("finalize", error))?;
        Ok(())
    })
}

fn transfer_error(
    phase: &str,
    error: crate::execution::errors::ExecutionError,
) -> crate::execution::errors::ExecutionError {
    crate::execution::errors::ExecutionError::new(
        error.kind,
        format!(
            "Remote dependency cache transfer could not {phase}: {}",
            error.message
        ),
        error.retryable,
    )
}
