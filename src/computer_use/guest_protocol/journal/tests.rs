use super::*;
use crate::computer_use::{
    ComputerActionActorV1, ComputerActionPayloadV1, ComputerOperationV1,
    guest_protocol::GuestActionReplyV1,
};
use std::sync::Arc;

const SESSION: &str = "00000000-0000-4000-8000-000000000001";
const REQUEST: &str = "00000000-0000-4000-8000-000000000002";

#[test]
fn pending_actions_survive_restart_as_uncertain_and_never_reexecute() {
    let temp = tempfile::tempdir().unwrap();
    let first_action = action(1, "00000000-0000-4000-8000-000000000003");
    let journal = open(temp.path());
    assert_eq!(
        journal.begin(REQUEST, &first_action).unwrap(),
        GuestReplayDecisionV1::Execute
    );
    drop(journal);

    let restarted = open(temp.path());
    assert_eq!(
        restarted.begin(REQUEST, &first_action),
        Err(GuestJournalErrorV1::OutcomeUnknown)
    );
}

#[test]
fn completed_duplicate_replays_exact_outcome_but_conflict_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let first_action = action(1, "00000000-0000-4000-8000-000000000003");
    let journal = open(temp.path());
    assert_eq!(
        journal.begin(REQUEST, &first_action).unwrap(),
        GuestReplayDecisionV1::Execute
    );
    let outcome = GuestActionOutcomeV1 {
        reply: GuestActionReplyV1 {
            action_sequence: 1,
            result: super::super::GuestOperationResultV1::Completed,
        },
        observation: None,
    };
    journal
        .complete(REQUEST, &first_action, outcome.clone())
        .unwrap();
    assert_eq!(
        journal.begin(REQUEST, &first_action).unwrap(),
        GuestReplayDecisionV1::Replay(outcome)
    );
    let conflicting = action(1, "00000000-0000-4000-8000-000000000004");
    assert_eq!(
        journal.begin(REQUEST, &conflicting),
        Err(GuestJournalErrorV1::ReplayConflict)
    );
}

#[test]
fn operator_inspection_reads_only_durable_action_disposition() {
    let temp = tempfile::tempdir().unwrap();
    let journal = open(temp.path());
    let action = action(1, "00000000-0000-4000-8000-000000000003");
    journal.begin(REQUEST, &action).unwrap();
    assert_eq!(
        journal.status_for_sequence(1).unwrap(),
        GuestActionJournalStatusV1::Pending
    );
    journal
        .complete(
            REQUEST,
            &action,
            GuestActionOutcomeV1 {
                reply: GuestActionReplyV1 {
                    action_sequence: 1,
                    result: super::super::GuestOperationResultV1::Failed,
                },
                observation: None,
            },
        )
        .unwrap();
    assert_eq!(
        journal.status_for_sequence(1).unwrap(),
        GuestActionJournalStatusV1::Completed(super::super::GuestOperationResultV1::Failed)
    );
    assert_eq!(
        journal.status_for_sequence(2).unwrap(),
        GuestActionJournalStatusV1::NotRecorded
    );
}

#[test]
fn concurrent_duplicate_intent_has_only_one_executor() {
    let temp = tempfile::tempdir().unwrap();
    let journal = Arc::new(open(temp.path()));
    let action = Arc::new(action(1, "00000000-0000-4000-8000-000000000003"));
    let contenders = (0..2)
        .map(|_| {
            let journal = Arc::clone(&journal);
            let action = Arc::clone(&action);
            std::thread::spawn(move || journal.begin(REQUEST, &action))
        })
        .collect::<Vec<_>>();
    let results = contenders
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .clone()
            .into_iter()
            .filter(|result| *result == Ok(GuestReplayDecisionV1::Execute))
            .count(),
        1
    );
    assert_eq!(
        results
            .into_iter()
            .filter(|result| *result == Err(GuestJournalErrorV1::OutcomeUnknown))
            .count(),
        1
    );
}

fn open(directory: &std::path::Path) -> GuestActionJournalV1 {
    #[cfg(unix)]
    std::fs::set_permissions(
        directory,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    GuestActionJournalV1::open(directory, SESSION.to_owned(), 1, 0, 8).unwrap()
}

fn action(sequence: u64, key: &str) -> ComputerActionV1 {
    ComputerActionV1 {
        schema_version: 1,
        session_id: SESSION.to_owned(),
        action_sequence: sequence,
        idempotency_key: key.to_owned(),
        actor: ComputerActionActorV1::Agent,
        lease_generation: 1,
        approval_generation: 1,
        cancellation_generation: 1,
        operation: ComputerOperationV1::ObserveScreen,
        payload: ComputerActionPayloadV1::ObserveScreen,
    }
}
