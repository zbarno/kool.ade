use super::*;

#[test]
fn remote_cache_transfer_validates_chunks_before_staging_snapshot() {
    use crate::execution::contract::DependencyCacheEcosystemV1;
    use crate::execution::remote::staging::{MAX_TRANSFER_CHUNK_BYTES, TransferChunkV1};

    let root = tempfile::tempdir().unwrap();
    let cache_root = tempfile::tempdir().unwrap();
    std::fs::write(
        cache_root.path().join("package.tgz"),
        b"verified package cache bytes",
    )
    .unwrap();
    let snapshot = crate::harness::resource_bridge::CacheSnapshot::capture(
        cache_root.path(),
        "00000000-0000-4000-8000-000000000001",
        DependencyCacheEcosystemV1::Npm,
        "00000000-0000-4000-8000-000000000002",
        "sha256:resolved",
        "x86_64",
        "00000000-0000-4000-8000-000000000003",
    )
    .unwrap();
    let transfer_id = "00000000-0000-4000-8000-000000000004";
    let backend_calls = Arc::new(AtomicUsize::new(0));
    let mut input = Vec::new();
    let mut sequence = 0;
    let mut request_kinds = Vec::new();
    {
        let mut send = |kind: EnvelopeKindV1, payload| {
            sequence += 1;
            request_kinds.push(kind.clone());
            let envelope = EnvelopeV1 {
                version: protocol::PROTOCOL_VERSION_V1,
                request_id: format!("00000000-0000-4000-8000-{:012}", sequence),
                execution_id: "00000000-0000-4000-8000-000000000000".into(),
                sequence,
                kind,
                payload,
            };
            protocol::write_frame(&mut input, &envelope).unwrap();
        };
        send(
            EnvelopeKindV1::BeginCacheTransfer,
            serde_json::json!({
                "transfer_id": transfer_id, "cache_ref": snapshot.cache_ref, "manifest": snapshot.manifest,
            }),
        );
        for (index, bytes) in snapshot
            .archive
            .chunks(MAX_TRANSFER_CHUNK_BYTES)
            .enumerate()
        {
            let chunk = TransferChunkV1::new(
                transfer_id,
                "cache.snapshot",
                (index * MAX_TRANSFER_CHUNK_BYTES) as u64,
                bytes.to_vec(),
            )
            .unwrap();
            send(
                EnvelopeKindV1::CacheTransferChunk,
                serde_json::json!({"chunk": chunk}),
            );
        }
        send(
            EnvelopeKindV1::FinalizeCacheTransfer,
            serde_json::json!({"transfer_id": transfer_id}),
        );
    }
    let output = serve_with_journal(
        Arc::new(FakeBackend(backend_calls.clone())),
        input,
        root.path(),
    );
    let mut frames = Cursor::new(output);
    let mut acknowledgements = 0;
    while let Some(response) = protocol::read_frame(&mut frames).unwrap() {
        assert_eq!(
            response.kind,
            EnvelopeKindV1::Acknowledge,
            "{:?}",
            response.payload
        );
        assert!(
            crate::execution::remote::client::matches_response(
                &request_kinds[acknowledgements],
                &response.kind,
            ),
            "controller must accept the authenticated cache-transfer response",
        );
        acknowledgements += 1;
    }
    assert!(acknowledgements >= 3);
    assert_eq!(backend_calls.load(Ordering::Relaxed), 1);
}

#[test]
fn worker_heartbeat_echoes_a_bounded_nonce() {
    let mut request = envelope("00000000-0000-4000-8000-000000000000");
    request.kind = EnvelopeKindV1::Heartbeat;
    request.payload = serde_json::json!({"nonce": "heartbeat-1"});
    let mut input = Vec::new();
    protocol::write_frame(&mut input, &request).unwrap();
    let root = tempfile::tempdir().unwrap();
    let output = serve_with_journal(
        Arc::new(FakeBackend(Arc::new(AtomicUsize::new(0)))),
        input,
        root.path(),
    );
    let response = protocol::read_frame(&mut Cursor::new(output))
        .unwrap()
        .unwrap();
    assert_eq!(response.kind, EnvelopeKindV1::Acknowledge);
    assert_eq!(response.payload, request.payload);
}

#[test]
fn reconnect_replays_the_durable_response_without_dispatching_twice() {
    let root = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(FakeBackend(calls.clone()));
    let request = envelope("00000000-0000-4000-8000-000000000000");
    let mut input = Vec::new();
    protocol::write_frame(&mut input, &request).unwrap();
    let first = serve_with_journal(backend.clone(), input.clone(), root.path());
    let second = serve_with_journal(backend, input, root.path());
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert_eq!(first, second);
}

#[test]
fn worker_rejects_invalid_heartbeat_payloads() {
    let mut request = envelope("00000000-0000-4000-8000-000000000000");
    request.kind = EnvelopeKindV1::Heartbeat;
    request.payload = serde_json::json!({"nonce": "", "unexpected": true});
    let mut input = Vec::new();
    protocol::write_frame(&mut input, &request).unwrap();
    let root = tempfile::tempdir().unwrap();
    let output = serve_with_journal(
        Arc::new(FakeBackend(Arc::new(AtomicUsize::new(0)))),
        input,
        root.path(),
    );
    let response = protocol::read_frame(&mut Cursor::new(output))
        .unwrap()
        .unwrap();
    assert_eq!(response.kind, EnvelopeKindV1::Error);
    assert_eq!(response.payload["kind"], "worker_incompatible");
}
