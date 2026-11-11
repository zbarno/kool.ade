use super::*;

#[test]
fn controller_recovery_store_reopens_uncertain_action_intent() {
    let root = tempfile::tempdir().unwrap();
    let key = signing_key();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let session = signed_session(&key, now);
    let path = root.path().join("recovery");
    let store = ComputerRemoteWorkerSessionStoreV1::open(path.clone()).unwrap();
    let record = ComputerRemoteWorkerRecoveryRecordV1 {
        schema_version: 1,
        worker_id: WORKER_ID.into(),
        machine_id: MACHINE_ID.into(),
        session: session.clone(),
        last_transport_sequence: 5,
        last_event_sequence: 2,
        pending_action_sequence: Some(1),
        outcome_unknown: true,
    };
    store.save(&record).unwrap();
    drop(store);
    let reopened = ComputerRemoteWorkerSessionStoreV1::open(path).unwrap();
    let recovered = reopened
        .load(&session.identity.session_id)
        .unwrap()
        .unwrap();
    assert_eq!(recovered, record);
    assert_eq!(recovered.pending_action_sequence, Some(1));
    assert!(recovered.outcome_unknown);
}
