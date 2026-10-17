use super::*;

fn checklist(plan_revision: u64, spec_revision: Option<&str>) -> ReadinessChecklistV1 {
    ReadinessChecklistV1 {
        feature_uid: uuid::Uuid::from_u128(1).to_string(),
        plan_revision,
        spec_revision: spec_revision.map(str::to_owned),
        required_requirement_ids: Some(vec!["REQ-1".into()]),
        assessed_axes: Vec::new(),
        acceptance_tests: Vec::new(),
        evidence_refs: Vec::new(),
        unresolved_blocker_ids: Vec::new(),
        blocker_notes: Vec::new(),
        ready: false,
        stalled: false,
        no_progress_count: 0,
        progress_fingerprint: None,
        revision: 0,
    }
}

#[test]
fn current_question_requires_matching_plan_and_spec_snapshots() {
    assert!(
        ensure_question_snapshot_is_current(
            4,
            &checklist(4, Some(&"a".repeat(64))),
            &"a".repeat(64),
            &"a".repeat(64),
        )
        .is_ok()
    );
    assert!(
        ensure_question_snapshot_is_current(
            4,
            &checklist(5, Some(&"a".repeat(64))),
            &"a".repeat(64),
            &"a".repeat(64),
        )
        .is_err()
    );
    assert!(
        ensure_question_snapshot_is_current(
            4,
            &checklist(4, None),
            &"a".repeat(64),
            &"a".repeat(64),
        )
        .is_err()
    );
    assert!(
        ensure_question_snapshot_is_current(
            4,
            &checklist(4, Some(&"a".repeat(64))),
            &"a".repeat(64),
            &"b".repeat(64),
        )
        .is_err()
    );
}
