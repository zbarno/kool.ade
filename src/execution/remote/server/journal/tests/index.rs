use super::*;

/// The index lifecycle is persisted at the cross-file trust boundary. Testing
/// state helpers alone cannot prove durable ordering; this uses a temporary
/// filesystem journal with no external services and low setup cost.
#[test]
fn index_records_reserved_pending_and_completed_transitions() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let request = request("00000000-0000-4000-8000-000000000001");
    assert_eq!(journal.reserve(&request).unwrap(), Lookup::New);
    let (_, mut index, _) = journal.open_index_locked().unwrap();
    let raw = read_index(&mut index).unwrap();
    assert!(
        raw.iter()
            .any(|entry| entry.state == RequestIndexState::Reserved)
    );
    assert!(
        raw.iter()
            .any(|entry| entry.state == RequestIndexState::Pending)
    );
    drop(index);

    let response = response(&request);
    journal.complete(&request, &response).unwrap();
    journal.complete(&request, &response).unwrap();
    let (_, mut index, _) = journal.open_index_locked().unwrap();
    let folded = super::super::index::fold(&read_index(&mut index).unwrap()).unwrap();
    let entry = folded
        .iter()
        .find(|entry| entry.request_id == request.request_id)
        .unwrap();
    assert!(
        matches!(&entry.state, RequestIndexState::Completed { completed_unix_ms } if *completed_unix_ms > 0)
    );
    assert_eq!(folded.len(), 1);
}

/// Models a process crash after the journal completion sync but before the
/// global index transition. The global fence must block cross-execution reuse
/// and an exact retry must repair the missing index state. Filesystem only.
#[test]
fn completed_journal_repairs_pending_index_after_recovery_window() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let request = request("00000000-0000-4000-8000-000000000001");
    assert_eq!(journal.reserve(&request).unwrap(), Lookup::New);
    let response = response(&request);
    let (lock, mut log, _) = journal.open_locked(&request.execution_id).unwrap();
    append_entry(
        &mut log,
        JournalEntry {
            request_id: request.request_id.clone(),
            request_sha256: fingerprint(&request).unwrap(),
            created_unix_ms: now_unix_ms(),
            state: EntryState::Completed {
                response: response.clone(),
            },
        },
    )
    .unwrap();
    FileExt::unlock(&lock).unwrap();

    let mut reused = request.clone();
    reused.execution_id = "00000000-0000-4000-8000-000000000004".into();
    assert_eq!(
        journal.reserve(&reused).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(journal.reserve(&request).unwrap(), Lookup::Replay(response));
    let (_, mut index, _) = journal.open_index_locked().unwrap();
    let folded = super::super::index::fold(&read_index(&mut index).unwrap()).unwrap();
    assert!(matches!(
        &folded[0].state,
        RequestIndexState::Completed { .. }
    ));
}

/// An impossible completed-index/pending-journal pair must fail closed without
/// changing the execution journal. This exercises the persisted filesystem
/// boundary; no external services are needed and setup is low cost.
#[test]
fn completed_index_with_pending_journal_is_rejected_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let request = request("00000000-0000-4000-8000-000000000001");
    assert_eq!(journal.reserve(&request).unwrap(), Lookup::New);

    let (lock, mut index, _) = journal.open_index_locked().unwrap();
    append_index(
        &mut index,
        RequestIndexEntry {
            request_id: request.request_id.clone(),
            execution_id: request.execution_id.clone(),
            request_sha256: fingerprint(&request).unwrap(),
            created_unix_ms: now_unix_ms(),
            state: RequestIndexState::Completed {
                completed_unix_ms: now_unix_ms(),
            },
        },
    )
    .unwrap();
    FileExt::unlock(&lock).unwrap();

    assert_eq!(
        journal
            .complete(&request, &response(&request))
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidData
    );
    let (lock, mut log, _) = journal.open_locked(&request.execution_id).unwrap();
    let entries = read_entries(&mut log).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0].state, EntryState::Pending));
    FileExt::unlock(&lock).unwrap();
}

/// Concurrent reserve retries racing completion exercise the production
/// index→execution lock boundary, which helper tests cannot establish. This
/// uses a temporary filesystem and threads, with no external services and low
/// setup cost.
#[test]
fn simultaneous_reserve_retries_and_completions_preserve_one_lifecycle() {
    const CALLERS: usize = 8;
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let request = request("00000000-0000-4000-8000-000000000001");
    assert_eq!(journal.reserve(&request).unwrap(), Lookup::New);
    let response = response(&request);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(CALLERS));
    let operations = (0..CALLERS)
        .map(|index| {
            let journal = journal.clone();
            let request = request.clone();
            let response = response.clone();
            let barrier = barrier.clone();
            let reserve_retry = index % 2 == 0;
            std::thread::spawn(move || {
                barrier.wait();
                if reserve_retry {
                    match journal.reserve(&request)? {
                        Lookup::Pending | Lookup::Replay(_) => Ok(()),
                        Lookup::New => Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "concurrent retry created a duplicate reservation",
                        )),
                        Lookup::Expired => Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "active request unexpectedly expired",
                        )),
                    }
                } else {
                    journal.complete(&request, &response)
                }
            })
        })
        .collect::<Vec<_>>();
    for thread in operations {
        thread.join().unwrap().unwrap();
    }
    assert_eq!(journal.reserve(&request).unwrap(), Lookup::Replay(response));
}

/// Pending work remains globally fenced even after its reservation timestamp
/// ages out. A per-execution check alone cannot prove cross-execution ID
/// uniqueness; this uses the persisted two-file boundary and no services.
#[test]
fn expired_pending_index_state_keeps_request_id_fenced() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let pending = request("00000000-0000-4000-8000-000000000001");
    let hash = fingerprint(&pending).unwrap();
    let (lock, mut log, _) = journal.open_locked(&pending.execution_id).unwrap();
    append_entry(
        &mut log,
        JournalEntry {
            request_id: pending.request_id.clone(),
            request_sha256: hash.clone(),
            created_unix_ms: 1,
            state: EntryState::Pending,
        },
    )
    .unwrap();
    FileExt::unlock(&lock).unwrap();
    let (lock, mut index, _) = journal.open_index_locked().unwrap();
    append_index(
        &mut index,
        RequestIndexEntry {
            request_id: pending.request_id.clone(),
            execution_id: pending.execution_id.clone(),
            request_sha256: hash,
            created_unix_ms: 1,
            state: RequestIndexState::Pending,
        },
    )
    .unwrap();
    FileExt::unlock(&lock).unwrap();

    assert_eq!(
        journal
            .reserve(&request("00000000-0000-4000-8000-000000000003"))
            .unwrap(),
        Lookup::New
    );
    let mut reuse = pending;
    reuse.execution_id = "00000000-0000-4000-8000-000000000004".into();
    assert_eq!(
        journal.reserve(&reuse).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[path = "index/migration.rs"]
mod migration;
