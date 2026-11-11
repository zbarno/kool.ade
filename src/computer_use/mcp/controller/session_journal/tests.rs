use super::*;

#[path = "tests/personal_auth.rs"]
mod personal_auth;

const SESSION: &str = "00000000-0000-4000-8000-000000000001";

#[test]
fn append_is_durable_ordered_bounded_and_restart_reconciliation_is_fenced() {
    let root = tempfile::tempdir().unwrap();
    let journal = ComputerUseSessionJournalV1::open(root.path()).unwrap();
    assert_eq!(
        journal
            .append(event(SessionEventKindV1::SessionRequested))
            .unwrap(),
        1
    );
    let mut action_intent = event(SessionEventKindV1::ActionIntent);
    action_intent.action_sequence = Some(1);
    action_intent.action_id = Some("00000000-0000-4000-8000-000000000002".into());
    action_intent.action_actor = Some(ComputerActionActorV1::Agent);
    action_intent.action_operation = Some(ComputerOperationV1::MouseInput);
    assert_eq!(journal.append(action_intent).unwrap(), 2);
    drop(journal);

    let restarted = ComputerUseSessionJournalV1::open(root.path()).unwrap();
    let recovered = restarted.reconcile_after_restart(42).unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].kind, SessionEventKindV1::SessionInterrupted);
    assert_eq!(
        recovered[0].reconciled_from,
        Some(SessionEventKindV1::ActionIntent)
    );
    assert_eq!(recovered[0].action_sequence, Some(1));
    let latest = restarted.latest_events().unwrap();
    assert_eq!(latest.len(), 1);
    assert_eq!(latest[0].sequence, 3);
    assert_eq!(latest[0].lease_generation, 4);

    restarted.dismiss(SESSION, 43).unwrap();
    assert!(restarted.reconcile_after_restart(44).unwrap().is_empty());
}

#[test]
fn journal_rejects_a_partial_tail_instead_of_guessing_the_last_state() {
    let root = tempfile::tempdir().unwrap();
    let journal = ComputerUseSessionJournalV1::open(root.path()).unwrap();
    journal
        .append(event(SessionEventKindV1::SessionRequested))
        .unwrap();
    drop(journal);
    let path = root.path().join(JOURNAL_FILE);
    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(b"{partial").unwrap();
    file.sync_all().unwrap();
    assert!(matches!(
        ComputerUseSessionJournalV1::open(root.path()),
        Err("session_journal_corrupt")
    ));
}

#[test]
fn persisted_events_contain_no_action_payload_or_observation_fields() {
    let root = tempfile::tempdir().unwrap();
    let journal = ComputerUseSessionJournalV1::open(root.path()).unwrap();
    let mut action_intent = event(SessionEventKindV1::ActionIntent);
    action_intent.action_sequence = Some(1);
    action_intent.action_id = Some("00000000-0000-4000-8000-000000000002".into());
    action_intent.action_actor = Some(ComputerActionActorV1::Human);
    action_intent.action_operation = Some(ComputerOperationV1::KeyboardInput);
    journal.append(action_intent).unwrap();
    let contents = std::fs::read_to_string(root.path().join(JOURNAL_FILE)).unwrap();
    assert!(!contents.contains("payload"));
    assert!(!contents.contains("observation"));
    assert!(!contents.contains("password"));
}

#[test]
fn action_intent_journal_enforces_durable_one_way_transitions_without_scope_data() {
    use ComputerActionIntentStateV1 as State;

    let root = tempfile::tempdir().unwrap();
    let journal = ComputerUseSessionJournalV1::open(root.path()).unwrap();
    let intent_id = "00000000-0000-4000-8000-000000000020";
    let digest = format!("sha256:{}", "d".repeat(64));
    for state in [
        State::Prepared,
        State::AwaitingApproval,
        State::ApprovedOnce,
        State::Executing,
        State::OutcomeUnknown,
    ] {
        journal
            .append(action_intent_state_event(intent_id, &digest, state))
            .unwrap();
    }
    assert!(
        journal
            .append(action_intent_state_event(
                intent_id,
                &digest,
                State::Prepared
            ))
            .is_err()
    );
    let contents = std::fs::read_to_string(root.path().join(JOURNAL_FILE)).unwrap();
    assert!(!contents.contains("content_preview"));
    assert!(!contents.contains("destination"));
    assert!(contents.contains("outcome_unknown"));
}

fn action_intent_state_event(
    intent_id: &str,
    digest: &str,
    state: ComputerActionIntentStateV1,
) -> SessionJournalEventV1 {
    let mut event = event(SessionEventKindV1::ActionIntentStateChanged);
    event.boot_fingerprint = None;
    event.action_intent_id = Some(intent_id.to_owned());
    event.action_intent_sha256 = Some(digest.to_owned());
    event.action_intent_state = Some(state);
    event
}

fn event(kind: SessionEventKindV1) -> SessionJournalEventV1 {
    SessionJournalEventV1 {
        schema_version: 1,
        sequence: 0,
        event_id: uuid::Uuid::new_v4().to_string(),
        kind,
        reconciled_from: None,
        boot_fingerprint: Some(format!("sha256:{}", "c".repeat(64))),
        identity: ComputerIdentityV1 {
            project_id: "00000000-0000-4000-8000-000000000010".into(),
            task_id: "00000000-0000-4000-8000-000000000011".into(),
            user_id: "00000000-0000-4000-8000-000000000012".into(),
            session_id: SESSION.into(),
        },
        session_class: ComputerSessionClassV1::Disposable,
        agent_role: "implementer/v1".into(),
        lease_id: "00000000-0000-4000-8000-000000000013".into(),
        lease_generation: 4,
        approval_generation: 2,
        cancellation_generation: 3,
        image_digest: format!("sha256:{}", "a".repeat(64)),
        policy_sha256: format!("sha256:{}", "b".repeat(64)),
        network_profile: ComputerNetworkProfileV1::Disabled,
        allowed_apps: vec!["chromium".into()],
        resource_budget: crate::computer_use::MINIMUM_DISPOSABLE_VM_BUDGET_V1,
        expires_at_unix_ms: 100,
        action_sequence: None,
        action_id: None,
        action_actor: None,
        action_operation: None,
        action_intent_id: None,
        action_intent_sha256: None,
        action_intent_state: None,
        occurred_at_unix_ms: 10,
    }
}
