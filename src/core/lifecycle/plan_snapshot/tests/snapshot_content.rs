use super::*;

#[test]
fn snapshot_is_stable_content_addressed_and_detects_edits() {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let first = PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    let repeated =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    assert_eq!(first, repeated);
    assert_eq!(first.requirements[0].stable_id, "REQ-001");
    assert_eq!(
        first.requirements[0].rationale,
        "Not specified in the source specification."
    );
    assert_eq!(first.requirements[1].stable_id.len(), 16);
    assert_eq!(first.non_goals, ["Out of scope: scheduled delivery"]);

    let changed = markdown.replace("valid file", "valid CSV file");
    let changed_contract = crate::core::workflow::feature_contract(&changed);
    let changed_snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &changed, &changed_contract).unwrap();
    assert_ne!(first.content_hash, changed_snapshot.content_hash);
    assert_ne!(first.uid, changed_snapshot.uid);

    let mut corrupt = first;
    corrupt.outcome.push_str(" changed");
    assert!(corrupt.validate().is_err());
}

#[test]
fn explicit_requirement_rationale_and_criteria_links_change_the_snapshot_hash() {
    let source = spec();
    let changed = source.replace(
        "REQ-001: Exports include selected rows.",
        "REQ-001: Exports include selected rows for AC-1. Rationale: operators need focused exports.",
    );
    let (identity, plain) = identified(&source);
    let (_, detailed) = identified(&changed);
    let original = PlanSnapshotV1::from_feature(
        "CHG-021",
        &identity,
        &plain,
        &crate::core::workflow::feature_contract(&plain),
    )
    .unwrap();
    let updated = PlanSnapshotV1::from_feature(
        "CHG-021",
        &identity,
        &detailed,
        &crate::core::workflow::feature_contract(&detailed),
    )
    .unwrap();
    assert_eq!(
        updated.requirements[0].rationale,
        "operators need focused exports"
    );
    assert_eq!(updated.requirements[0].acceptance_criteria_ids, ["AC-1"]);
    assert_ne!(original.content_hash, updated.content_hash);
}

#[test]
fn empty_explicit_requirement_id_uses_a_deterministic_generated_id() {
    let malformed = spec().replace("REQ-001: Exports", "REQ-: Exports");
    let (identity, markdown) = identified(&malformed);
    let contract = crate::core::workflow::feature_contract(&markdown);
    let first = PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    let again = PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    assert_ne!(first.requirements[0].stable_id, "REQ-");
    assert!(first.requirements[0].stable_id.starts_with("REQ-"));
    assert_eq!(first.requirements[0].stable_id.len(), 16);
    assert_eq!(
        first.requirements[0].stable_id,
        again.requirements[0].stable_id
    );
}
