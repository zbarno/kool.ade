use super::*;

#[test]
fn personal_synthetic_login_takeover_and_restart_journal_contains_no_secrets() {
    const PASSWORD: &str = "synthetic-only-password-7f3c";
    const MFA_CODE: &str = "000731";
    let root = tempfile::tempdir().unwrap();
    let journal = ComputerUseSessionJournalV1::open(root.path()).unwrap();
    let mut takeover = event(SessionEventKindV1::HumanTakeover);
    takeover.session_class = ComputerSessionClassV1::PersonalPersistent;
    journal.append(takeover).unwrap();
    let mut handback = event(SessionEventKindV1::AgentResumed);
    handback.session_class = ComputerSessionClassV1::PersonalPersistent;
    journal.append(handback).unwrap();
    drop(journal);

    let restarted = ComputerUseSessionJournalV1::open(root.path()).unwrap();
    let recovered = restarted.reconcile_after_restart(42).unwrap();
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].kind, SessionEventKindV1::SessionInterrupted);
    assert_eq!(
        recovered[0].reconciled_from,
        Some(SessionEventKindV1::AgentResumed)
    );

    let serialized = std::fs::read_to_string(root.path().join(JOURNAL_FILE)).unwrap();
    assert!(!serialized.contains(PASSWORD));
    assert!(!serialized.contains(MFA_CODE));
    assert!(!serialized.contains("fixture_session"));
}
