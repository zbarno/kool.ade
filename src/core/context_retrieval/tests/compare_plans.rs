use super::*;

#[test]
fn compare_plans_context_names_target_and_forbids_operator_authored_alternatives() {
    let (mut state, root) = fixture();
    state.active_feature = Some(("F7".into(), "# F7: Compare plans\n".into()));
    state.active_features = state.active_feature.clone().into_iter().collect();
    let prompt = crate::core::prompt::workflow_context_for_turn(
        &state,
        crate::core::workflow::TurnPurpose::ComparePlans,
        Some("F7"),
    );
    assert!(prompt.contains("COMPARE PLANS"));
    assert!(prompt.contains("Compared feature: F7"));
    assert!(prompt.contains("Kool.ad/e authors both alternatives"));
    assert!(prompt.contains("sequencing, boundaries, and rollback"));
    assert!(prompt.contains("advisory: state that the operator makes the final choice"));
    assert!(prompt.contains("No application actions are permitted"));
    assert!(!prompt.contains("Valid action names:"));
    let _ = std::fs::remove_dir_all(root);
}
