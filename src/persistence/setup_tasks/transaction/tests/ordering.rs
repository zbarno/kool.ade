use super::*;

#[test]
fn a_later_reserved_generation_fences_an_older_scan_that_finishes_last() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.json");
    let slow_generation = begin_scan_at(&path).unwrap();
    let fast_generation = begin_scan_at(&path).unwrap();

    let fast = snapshot(fast_generation, "newer reserved scan");
    reconcile_live_at(&path, &fast, fast_generation).unwrap();

    let slow = snapshot(slow_generation, "older reserved scan");
    assert!(reconcile_live_at(&path, &slow, slow_generation).is_err());
    let store = load_from(&path).unwrap();
    assert_eq!(
        store.tasks["setup:docker:rootless"].title,
        "newer reserved scan"
    );
}

#[test]
/// Production boundary: persisted task data survives store reopen and repeated
/// scan reconciliation without duplicate cards. An in-memory unit test cannot
/// prove serialization/reopen behavior. Uses a temporary file only; setup is
/// negligible and has no external service dependency.
fn computer_setup_rescans_reconcile_one_persisted_card_without_duplicates() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("tasks.json");
    let first_generation = begin_scan_at(&path).unwrap();
    let mut first = snapshot(first_generation, "first scan");
    first
        .findings
        .push(crate::app::setup_attention::catalog::Finding {
            id: "setup:computer:kvm".into(),
            title: "Enable KVM for disposable computers".into(),
            explanation: "The KVM device is unavailable.".into(),
            next_step: "Check firmware virtualization and rescan.".into(),
            status: crate::app::setup_attention::catalog::FindingStatus::NeedsAttention,
            prerequisites: Vec::new(),
        });
    reconcile_live_at(&path, &first, first_generation).unwrap();

    let reopened = load_from(&path).unwrap();
    assert!(reopened.tasks.contains_key("setup:computer:kvm"));
    let created_at = reopened.tasks["setup:computer:kvm"].created_at_unix_ms;

    let second_generation = begin_scan_at(&path).unwrap();
    let mut second = snapshot(second_generation, "second scan");
    second.findings.push(first.findings[1].clone());
    reconcile_live_at(&path, &second, second_generation).unwrap();

    let reopened = load_from(&path).unwrap();
    assert_eq!(
        reopened
            .tasks
            .keys()
            .filter(|id| id.as_str() == "setup:computer:kvm")
            .count(),
        1
    );
    assert_eq!(
        reopened.tasks["setup:computer:kvm"].created_at_unix_ms,
        created_at
    );
    assert_eq!(
        reopened.tasks["setup:computer:kvm"].scan_generation,
        second_generation
    );
}
