use super::*;

#[test]
fn completed_start_replays_its_handle_after_a_lost_worker_reply() {
    // Production boundary: sends one stable Start envelope through two worker
    // server lifetimes sharing the durable journal, simulating a lost first
    // reply and controller replay. Journal unit tests cannot prove Start
    // dispatch avoids reserving capacity twice. Fake backend and temp files;
    // no Docker, SSH, provider, or network setup.
    let root = tempfile::tempdir().unwrap();
    let execution_id = "00000000-0000-4000-8000-000000000147";
    let calls = Arc::new(AtomicUsize::new(0));
    let (lease, mut request) = super::start_request(execution_id);
    request.cancellation_generation = 1;
    let mut start = super::envelope(execution_id);
    start.request_id = execution_id.into();
    start.sequence = 7;
    start.kind = EnvelopeKindV1::Start;
    start.payload = serde_json::json!({"lease": lease, "request": request});
    let mut input = Vec::new();
    protocol::write_frame(&mut input, &start).unwrap();

    let _lost_first_reply = super::serve_with_journal(
        Arc::new(super::super::FakeBackend(calls.clone())),
        input.clone(),
        root.path(),
    );
    let replay = super::serve_with_journal(
        Arc::new(super::super::FakeBackend(calls.clone())),
        input,
        root.path(),
    );

    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let response = protocol::read_frame(&mut Cursor::new(replay))
        .unwrap()
        .unwrap();
    assert_eq!(response.kind, EnvelopeKindV1::Acknowledge);
    assert_eq!(response.request_id, execution_id);
    assert_eq!(response.sequence, 7);
    let handle: ExecutionHandleV1 = serde_json::from_value(response.payload).unwrap();
    assert_eq!(handle.execution_id, execution_id);
    assert_eq!(handle.task_id, "00000000-0000-4000-8000-000000000144");
}
