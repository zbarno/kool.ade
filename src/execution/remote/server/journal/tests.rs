use super::*;
use crate::execution::remote::protocol::{EnvelopeKindV1, PROTOCOL_VERSION_V1};

fn request(id: &str) -> EnvelopeV1 {
    EnvelopeV1 {
        version: PROTOCOL_VERSION_V1,
        request_id: id.into(),
        execution_id: "00000000-0000-4000-8000-000000000002".into(),
        sequence: 1,
        kind: EnvelopeKindV1::Health,
        payload: serde_json::json!({"wire_version": PROTOCOL_VERSION_V1}),
    }
}

fn response(request: &EnvelopeV1) -> EnvelopeV1 {
    EnvelopeV1 {
        kind: EnvelopeKindV1::Acknowledge,
        payload: serde_json::json!({"ok": true}),
        ..request.clone()
    }
}

#[test]
fn exact_completed_request_replays_saved_response_without_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let request = request("00000000-0000-4000-8000-000000000001");
    assert_eq!(journal.reserve(&request).unwrap(), Lookup::New);
    assert_eq!(journal.reserve(&request).unwrap(), Lookup::Pending);
    let response = response(&request);
    journal.complete(&request, &response).unwrap();
    assert_eq!(journal.reserve(&request).unwrap(), Lookup::Replay(response));
}

#[test]
fn conflicting_request_id_and_corrupt_journal_fail_closed() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let request = request("00000000-0000-4000-8000-000000000001");
    assert_eq!(journal.reserve(&request).unwrap(), Lookup::New);
    let mut conflict = request.clone();
    conflict.payload = serde_json::json!({"wire_version": 2});
    assert_eq!(
        journal.reserve(&conflict).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    let mut other_execution = request.clone();
    other_execution.execution_id = "00000000-0000-4000-8000-000000000003".into();
    assert_eq!(
        journal.reserve(&other_execution).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    let path = root.path().join(&request.execution_id).join("rpc.jsonl");
    std::fs::write(path, b"{truncated\n").unwrap();
    assert_eq!(
        journal.reserve(&request).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn expired_requests_are_rejected_without_becoming_new_work() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let request = request("00000000-0000-4000-8000-000000000001");
    let (lock, mut log) = journal.open_locked(&request.execution_id).unwrap();
    append_entry(
        &mut log,
        JournalEntry {
            request_id: request.request_id.clone(),
            request_sha256: fingerprint(&request).unwrap(),
            created_unix_ms: 1,
            state: EntryState::Pending,
        },
    )
    .unwrap();
    FileExt::unlock(&lock).unwrap();
    assert_eq!(journal.reserve(&request).unwrap(), Lookup::Expired);
}
