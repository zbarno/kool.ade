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
fn schema_two_snapshot_keeps_legacy_capability_and_hash_contract() {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.schema_version = 2;
    snapshot.capability_id = Some("self-improvement-lifecycle".into());
    snapshot.acceptance_criteria = vec![
        "AC-001: Selected rows are exported.".into(),
        "AC-002: Empty results produce a valid file.".into(),
    ];
    snapshot.requirements[0].rationale.clear();
    snapshot.requirements[0].acceptance_criteria_ids.clear();
    snapshot.original_goal.clear();
    snapshot.content_hash = hash::content_hash(&snapshot).unwrap();
    snapshot.uid = snapshot_uid(&snapshot.feature_uid, &snapshot.content_hash).unwrap();
    snapshot.validate().unwrap();

    let hash = snapshot.content_hash.clone();
    let uid = snapshot.uid.clone();
    snapshot.original_goal = "ignored by the schema 2 content hash".into();
    assert_eq!(hash::content_hash(&snapshot).unwrap(), hash);
    assert!(
        snapshot.validate().is_err(),
        "schema 2 must reject unbound original-goal provenance"
    );
    snapshot.original_goal.clear();

    let mut persisted = serde_json::to_value(&snapshot).unwrap();
    let requirement = persisted["requirements"][0].as_object_mut().unwrap();
    requirement.remove("acceptanceCriteriaIds");
    requirement.insert("acIds".into(), serde_json::json!([]));
    requirement.insert("rationale".into(), serde_json::Value::Null);
    let loaded: PlanSnapshotV1 = serde_json::from_value(persisted).unwrap();
    assert_eq!(loaded.uid, uid);
    loaded.validate().unwrap();
    let mut tampered = loaded;
    tampered.capability_id = Some("other-capability".into());
    assert!(tampered.validate().is_err());
}

#[test]
fn schema_two_snapshot_rejects_malformed_criteria_and_dangling_requirement_refs() {
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.schema_version = 2;
    snapshot.capability_id = Some("self-improvement-lifecycle".into());
    snapshot.acceptance_criteria = vec![
        "AC-001: Selected rows are exported.".into(),
        "AC-002: Empty results produce a valid file.".into(),
    ];
    snapshot.requirements[0].acceptance_criteria_ids = vec!["AC-001".into()];
    snapshot.original_goal.clear();

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
