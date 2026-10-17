use super::super::*;

#[test]
fn repeated_identical_passes_eventually_stall_without_question_count_limit() {
    let mut state = super::checklist();
    assert!(!observe_progress(&mut state, &[], 2));
    assert!(!observe_progress(&mut state, &[], 2));
    assert!(observe_progress(&mut state, &[], 2));
    assert!(matches!(
        assess_readiness(&[], &state, &["REQ-1".into()]),
        ReadinessResult::PlanningStalled { .. }
    ));
}

#[test]
fn spec_revision_change_counts_as_progress_and_resets_no_progress_budget() {
    let mut state = super::checklist();
    assert!(!observe_progress(&mut state, &[], 2));
    assert!(!observe_progress(&mut state, &[], 2));
    assert_eq!(state.no_progress_count, 1);

    state.spec_revision = Some("b".repeat(64));

    assert!(!observe_progress(&mut state, &[], 2));
    assert_eq!(state.no_progress_count, 0);
    assert!(!state.stalled);
}
