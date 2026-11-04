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
fn current_schema_binds_capability_goal_and_requirement_details() {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.capability_id = Some("self-improvement-lifecycle".into());
    snapshot.acceptance_criteria = vec![
        "AC-001: Selected rows are exported.".into(),
        "AC-002: Empty results produce a valid file.".into(),
    ];
    snapshot.content_hash = hash::content_hash(&snapshot).unwrap();
    snapshot.uid = snapshot_uid(&snapshot.feature_uid, &snapshot.content_hash).unwrap();
    snapshot.validate().unwrap();

    let hash = snapshot.content_hash.clone();
    let uid = snapshot.uid.clone();
    snapshot.original_goal = "A distinct, bound product request.".into();
    assert_ne!(hash::content_hash(&snapshot).unwrap(), hash);
    snapshot.original_goal.clear();

    snapshot.requirements[0].rationale = "A different rationale.".into();
    assert_ne!(hash::content_hash(&snapshot).unwrap(), hash);
    snapshot.requirements[0].rationale = "Not specified in the source specification.".into();

    snapshot.capability_id = Some("self-improvement-lifecycle".into());
    assert_eq!(hash::content_hash(&snapshot).unwrap(), hash);
    assert_eq!(snapshot.uid, uid);
    let mut tampered = snapshot;
    tampered.capability_id = Some("other-capability".into());
    assert!(tampered.validate().is_err());
}

#[test]
fn current_schema_rejects_malformed_criteria_and_dangling_requirement_refs() {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.capability_id = Some("self-improvement-lifecycle".into());
    snapshot.acceptance_criteria = vec![
        "AC-001: Selected rows are exported.".into(),
        "AC-002: Empty results produce a valid file.".into(),
    ];
    snapshot.requirements[0].acceptance_criteria_ids = vec!["AC-001".into()];

    let mut duplicate = snapshot.clone();
    duplicate.acceptance_criteria[1] = "AC-001: Empty results produce a valid file.".into();
    assert!(reseal(duplicate).validate().is_err());

    let mut malformed = snapshot.clone();
    malformed.acceptance_criteria[0] = "AC001: Selected rows are exported.".into();
    assert!(reseal(malformed.clone()).validate().is_err());
    malformed.acceptance_criteria[0] = "AC-1: Selected rows are exported.".into();
    assert!(reseal(malformed.clone()).validate().is_err());
    malformed.acceptance_criteria[0] = "AC 001: Selected rows are exported.".into();
    assert!(reseal(malformed).validate().is_err());

    let mut dangling = snapshot;
    dangling.requirements[0]
        .acceptance_criteria_ids
        .push("AC-999".into());
    assert!(reseal(dangling).validate().is_err());
}

#[test]
fn old_schema_numbers_cannot_be_resealed_as_current_snapshots() {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let current = PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();

    for old_schema_version in [1, 2] {
        let mut old = current.clone();
        old.schema_version = old_schema_version;
        let old = reseal(old);
        assert!(old.validate().is_err());
    }
}

#[test]
fn old_requirement_wire_fields_are_rejected() {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    let mut old_wire = serde_json::to_value(snapshot).unwrap();
    old_wire.as_object_mut().unwrap().remove("originalGoal");
    let requirement = old_wire["requirements"][0].as_object_mut().unwrap();
    requirement.remove("rationale");
    requirement.insert("acIds".into(), serde_json::json!([]));

    assert!(serde_json::from_value::<PlanSnapshotV1>(old_wire).is_err());
}

fn reseal(mut snapshot: PlanSnapshotV1) -> PlanSnapshotV1 {
    snapshot.content_hash = hash::content_hash(&snapshot).unwrap();
    snapshot.uid = snapshot_uid(&snapshot.feature_uid, &snapshot.content_hash).unwrap();
    snapshot
}

#[test]
fn explicit_requirement_rationale_and_criteria_links_change_the_snapshot_hash() {
    let source = spec();
    let changed = source.replace(
        "REQ-001: Exports include selected rows.",
        "REQ-001: Exports include selected rows for AC-001. Rationale: operators need focused exports.",
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
    assert_eq!(updated.requirements[0].acceptance_criteria_ids, ["AC-001"]);
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
