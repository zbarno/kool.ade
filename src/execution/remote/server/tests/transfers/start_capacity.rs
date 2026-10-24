use super::*;

#[test]
fn worker_capacity_rejection_is_explicitly_pre_start_over_its_rpc_boundary() {
    // Production boundary: drives a framed Start through the real worker
    // dispatcher with every host slot held. A capacity-pool unit test cannot
    // prove the response kind/message used by controller rollback or that the
    // backend was not called. Setup uses local lock files and temp dirs; no
    // Docker daemon or SSH worker is needed.
    let root = tempfile::tempdir().unwrap();
    let worker_id = "00000000-0000-4000-8000-000000000141";
    let execution_id = "00000000-0000-4000-8000-000000000142";
    let pool =
        crate::execution::remote::WorkerCapacityPool::new(root.path(), worker_id.into()).unwrap();
    let occupied = (0..pool.max_jobs())
        .map(|_| pool.acquire().unwrap())
        .collect::<Vec<_>>();
    let (lease, request) = super::start_request(execution_id);
    let mut envelope = super::envelope(execution_id);
    envelope.kind = EnvelopeKindV1::Start;
    envelope.payload = serde_json::json!({"lease": lease, "request": request});
    let mut input = Vec::new();
    protocol::write_frame(&mut input, &envelope).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let mut output = Vec::new();
    super::super::serve_stdio_with_journal(
        Arc::new(super::super::FakeBackend(calls.clone())),
        None,
        Some(Arc::new(pool)),
        Cursor::new(input),
        &mut output,
        super::super::super::journal::RequestJournal::new(root.path().join("journal")),
    )
    .unwrap();
    drop(occupied);

    let response = protocol::read_frame(&mut Cursor::new(output))
        .unwrap()
        .unwrap();
    assert_eq!(response.kind, EnvelopeKindV1::Error);
    assert_eq!(response.payload["kind"], "worker_incompatible");
    assert_eq!(
        response.payload["message"],
        crate::execution::errors::REMOTE_START_CAPACITY_REJECTION_MESSAGE
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
}
