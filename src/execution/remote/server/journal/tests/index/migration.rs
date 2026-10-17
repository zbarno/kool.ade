use super::*;

/// A legacy reservation with a recent completion remains fenced for the full
/// completion retention interval. This exercises file-backed migration from
/// the old index schema into the persisted Completed state.
#[test]
fn legacy_completed_row_uses_completion_time_for_retention() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let old = request("00000000-0000-4000-8000-000000000001");
    let hash = fingerprint(&old).unwrap();
    let completed_at = now_unix_ms();
    let (lock, mut log, _) = journal.open_locked(&old.execution_id).unwrap();
    append_entry(
        &mut log,
        JournalEntry {
            request_id: old.request_id.clone(),
            request_sha256: hash.clone(),
            created_unix_ms: completed_at,
            state: EntryState::Completed {
                response: response(&old),
            },
        },
    )
    .unwrap();
    FileExt::unlock(&lock).unwrap();
    let legacy = serde_json::json!({
        "request_id": &old.request_id,
        "execution_id": &old.execution_id,
        "request_sha256": hash,
        "created_unix_ms": 1
    });
    let mut bytes = serde_json::to_vec(&legacy).unwrap();
    bytes.push(b'\n');
    std::fs::write(root.path().join("request-index.jsonl"), bytes).unwrap();

    assert_eq!(
        journal
            .reserve(&request("00000000-0000-4000-8000-000000000003"))
            .unwrap(),
        Lookup::New
    );
    let mut reuse = old.clone();
    reuse.execution_id = "00000000-0000-4000-8000-000000000004".into();
    assert_eq!(
        journal.reserve(&reuse).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    let (_, mut index, _) = journal.open_index_locked().unwrap();
    let folded = super::super::super::index::fold(&read_index(&mut index).unwrap()).unwrap();
    assert!(folded.iter().any(|entry| entry.request_id == old.request_id
        && entry.state
            == (RequestIndexState::Completed {
                completed_unix_ms: completed_at
            })));
}

/// Completed reservations leave the global index after the retention period
/// measured from completion. This filesystem-backed case also proves an expired
/// identity can then be reused under the documented retention contract.
#[test]
fn completed_index_state_expires_from_completion_timestamp() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let old = request("00000000-0000-4000-8000-000000000001");
    let hash = fingerprint(&old).unwrap();
    let (lock, mut log, _) = journal.open_locked(&old.execution_id).unwrap();
    append_entry(
        &mut log,
        JournalEntry {
            request_id: old.request_id.clone(),
            request_sha256: hash.clone(),
            created_unix_ms: 1,
            state: EntryState::Completed {
                response: response(&old),
            },
        },
    )
    .unwrap();
    FileExt::unlock(&lock).unwrap();
    let (lock, mut index, _) = journal.open_index_locked().unwrap();
    append_index(
        &mut index,
        RequestIndexEntry {
            request_id: old.request_id.clone(),
            execution_id: old.execution_id.clone(),
            request_sha256: hash,
            created_unix_ms: 1,
            state: RequestIndexState::Completed {
                completed_unix_ms: 1,
            },
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
    let mut reuse = old;
    reuse.execution_id = "00000000-0000-4000-8000-000000000004".into();
    assert_eq!(journal.reserve(&reuse).unwrap(), Lookup::New);
}

/// One reservation migrates no more than a bounded batch of old rows. This
/// uses serialized legacy JSON and a temp index to cover the migration format.
#[test]
fn legacy_index_migration_processes_one_execution_per_reservation() {
    let root = tempfile::tempdir().unwrap();
    let journal = RequestJournal::new(root.path().to_owned());
    let first_execution = "00000000-0000-4000-8000-000000000002";
    let second_execution = "00000000-0000-4000-8000-000000000004";
    let mut contents = Vec::new();
    for _ in 0..300 {
        let request_id = uuid::Uuid::new_v4().hyphenated().to_string();
        serde_json::to_writer(
            &mut contents,
            &serde_json::json!({
                "request_id": request_id,
                "execution_id": first_execution,
                "request_sha256": "a".repeat(64),
                "created_unix_ms": 1
            }),
        )
        .unwrap();
        contents.push(b'\n');
    }
    for _ in 0..20 {
        serde_json::to_writer(
            &mut contents,
            &serde_json::json!({
                "request_id": uuid::Uuid::new_v4().hyphenated().to_string(),
                "execution_id": second_execution,
                "request_sha256": "b".repeat(64),
                "created_unix_ms": 2
            }),
        )
        .unwrap();
        contents.push(b'\n');
    }
    std::fs::write(root.path().join("request-index.jsonl"), contents).unwrap();
    let next = request("00000000-0000-4000-8000-000000000003");
    assert_eq!(journal.reserve(&next).unwrap(), Lookup::New);
    let (_, mut index, _) = journal.open_index_locked().unwrap();
    let folded = super::super::super::index::fold(&read_index(&mut index).unwrap()).unwrap();
    assert_eq!(folded.len(), 21);
    assert_eq!(
        folded
            .iter()
            .filter(|entry| entry.execution_id == second_execution)
            .count(),
        20
    );
    assert_eq!(
        folded
            .iter()
            .filter(|entry| entry.state == RequestIndexState::Unknown)
            .count(),
        20
    );
    drop(index);

    let second = request("00000000-0000-4000-8000-000000000005");
    assert_eq!(journal.reserve(&second).unwrap(), Lookup::New);
    let (_, mut index, _) = journal.open_index_locked().unwrap();
    let folded = super::super::super::index::fold(&read_index(&mut index).unwrap()).unwrap();
    assert_eq!(folded.len(), 2);
    assert!(
        folded
            .iter()
            .all(|entry| entry.state != RequestIndexState::Unknown)
    );
}

/// Migration must fail closed on symlinked execution storage instead of
/// following it while classifying stale reservations. Local filesystem only.
#[cfg(unix)]
#[test]
fn legacy_migration_rejects_execution_journal_symlinks() {
    for symlink_log in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let journal = RequestJournal::new(root.path().to_owned());
        let expired = request("00000000-0000-4000-8000-000000000001");
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
        let target = root.path().join("outside-journal");
        if symlink_log {
            let execution = root.path().join(&expired.execution_id);
            std::fs::create_dir(&execution).unwrap();
            std::fs::write(&target, b"outside").unwrap();
            std::os::unix::fs::symlink(target, execution.join("rpc.jsonl")).unwrap();
        } else {
            std::os::unix::fs::symlink(target, root.path().join(&expired.execution_id)).unwrap();
        }
        assert_eq!(
            journal
                .reserve(&request("00000000-0000-4000-8000-000000000003"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
