use super::*;

#[test]
fn specification_requirements_without_questions_still_need_acceptance_test_traces() {
    let mut state = super::checklist();
    state.required_requirement_ids = Some(vec!["REQ-1".into(), "REQ-2".into()]);
    assert!(matches!(
        assess_readiness(&[], &state, &["REQ-1".into(), "REQ-2".into()]),
        ReadinessResult::NeedsWork {
            untraced_requirements,
            ..
        } if untraced_requirements == ["REQ-2"]
    ));
}

#[test]
fn legacy_readiness_without_specification_requirement_snapshot_fails_closed() {
    let mut state = super::checklist();
    state.required_requirement_ids = None;
    assert!(matches!(
        assess_readiness(&[], &state, &["REQ-1".into()]),
        ReadinessResult::InvalidEvidence { .. }
    ));
}
