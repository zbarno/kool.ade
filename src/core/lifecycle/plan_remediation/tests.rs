use super::*;

fn remediation() -> PlanRemediationV1 {
    PlanRemediationV1 {
        schema_version: 1,
        feature_uid: "00000000-0000-4000-8000-000000000001".into(),
        snapshot_uid: "00000000-0000-4000-8000-000000000002".into(),
        snapshot_hash: "a".repeat(64),
        review_receipt_uid: "00000000-0000-4000-8000-000000000003".into(),
        dispositions: vec![PlanFindingDispositionV1 {
            finding_id: "F-1".into(),
            action: PlanFindingActionV1::RepairPlan {
                rationale: "Clarifies the missing failure behavior.".into(),
            },
        }],
    }
}

#[test]
fn remediation_requires_valid_identity_and_unique_finding_actions() {
    remediation().validate_shape().unwrap();

    let mut invalid = remediation();
    invalid.snapshot_hash = "not-a-hash".into();
    assert!(invalid.validate_shape().is_err());

    let mut duplicate = remediation();
    duplicate
        .dispositions
        .push(duplicate.dispositions[0].clone());
    assert!(duplicate.validate_shape().is_err());
}

#[test]
fn contesting_a_finding_requires_a_reason_and_evidence_refs() {
    let mut value = remediation();
    value.dispositions[0].action = PlanFindingActionV1::ContestFinding {
        reason: "The cited workflow is outside this feature's scope.".into(),
        evidence_refs: vec!["spec:feature:CHG-021".into()],
    };
    value.validate_shape().unwrap();

    if let PlanFindingActionV1::ContestFinding { evidence_refs, .. } =
        &mut value.dispositions[0].action
    {
        evidence_refs.clear();
    }
    assert!(value.validate_shape().is_err());
}
