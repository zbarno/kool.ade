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
    let (lock, mut log, _) = journal.open_locked(&request.execution_id).unwrap();
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

#[test]
fn reserving_new_work_compacts_expired_entries_and_reclaims_capacity() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let expired = request("00000000-0000-4000-8000-000000000001");
    let (lock, mut log, _) = journal.open_locked(&expired.execution_id).unwrap();
    append_entry(
        &mut log,
        JournalEntry {
            request_id: expired.request_id.clone(),
            request_sha256: fingerprint(&expired).unwrap(),
            created_unix_ms: 1,
            state: EntryState::Completed {
                response: response(&expired),
            },
        },
    )
    .unwrap();
    FileExt::unlock(&lock).unwrap();
    let (lock, mut index, _) = journal.open_index_locked().unwrap();
    append_index(
        &mut index,
        RequestIndexEntry {
            request_id: expired.request_id.clone(),
            execution_id: expired.execution_id.clone(),
            request_sha256: fingerprint(&expired).unwrap(),
            created_unix_ms: 1,
            state: RequestIndexState::Unknown,
        },
    )
    .unwrap();
    FileExt::unlock(&lock).unwrap();

    let next = EnvelopeV1 {
        request_id: "00000000-0000-4000-8000-000000000003".into(),
        ..expired.clone()
    };
    assert_eq!(journal.reserve(&next).unwrap(), Lookup::New);
    let (_, mut index, _) = journal.open_index_locked().unwrap();
    assert_eq!(
        super::index::fold(&read_index(&mut index).unwrap())
            .unwrap()
            .len(),
        1
    );
    let (_, mut log, _) = journal.open_locked(&next.execution_id).unwrap();
    assert_eq!(read_entries(&mut log).unwrap().len(), 1);
}

#[test]
fn full_expired_request_index_is_compacted_before_capacity_is_checked() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let index_path = root.path().join("request-index.jsonl");
    let mut contents = Vec::new();
    let absent_execution = uuid::Uuid::new_v4().hyphenated().to_string();
    for _ in 0..MAX_JOURNAL_ENTRIES {
        let entry = RequestIndexEntry {
            request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            execution_id: absent_execution.clone(),
            request_sha256: "a".repeat(64),
            created_unix_ms: 1,
            state: RequestIndexState::Unknown,
        };
        serde_json::to_writer(&mut contents, &entry).unwrap();
        contents.push(b'\n');
    }
    std::fs::write(index_path, contents).unwrap();
    let next = request("00000000-0000-4000-8000-000000000009");
    assert_eq!(journal.reserve(&next).unwrap(), Lookup::New);
    let (_, mut index, _) = journal.open_index_locked().unwrap();
    let folded = super::index::fold(&read_index(&mut index).unwrap()).unwrap();
    assert_eq!(folded.len(), 1);
    assert!(
        folded
            .iter()
            .any(|entry| entry.request_id == next.request_id)
    );
    assert!(!root.path().join(absent_execution).exists());
}

#[path = "tests/index.rs"]
mod index_tests;
