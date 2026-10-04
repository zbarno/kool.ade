use super::feature_contract;

#[test]
fn selected_plan_changes_the_approval_contract_fingerprint() {
    let feature = "# CHG-010: Example\n\n## Intent\n\nShip the feature.\n\n## Selected Plan\n\nAlt A rollout.\n";
    let adopted_a = feature_contract(feature);
    let adopted_b = feature_contract(&feature.replace("Alt A rollout", "Alt B rollout"));
    let discarded = feature_contract(&feature.replace("## Selected Plan\n\nAlt A rollout.\n", ""));
    assert_ne!(adopted_a, adopted_b);
    assert_ne!(adopted_a, discarded);
    assert_ne!(adopted_b, discarded);
}
