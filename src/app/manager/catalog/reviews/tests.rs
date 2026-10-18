use super::*;

fn target() -> std::collections::HashMap<String, ReviewTarget> {
    [(
        "plan-review:snapshot-1".into(),
        ReviewTarget {
            feature_id: "CHG-004".into(),
            revision: "a".repeat(64),
            finding_ids: vec!["F-1".into(), "F-2".into()],
            finding_claims: [
                (
                    "F-1".into(),
                    "the finding location: persisted correction".into(),
                ),
                (
                    "F-2".into(),
                    "the second location: second correction".into(),
                ),
            ]
            .into(),
            evidence_ids: vec!["product-spec:architecture-and-constraints".into()],
        },
    )]
    .into()
}

fn action(kind: ManagerActionKind, finding: &str) -> ManagerActionV1 {
    ManagerActionV1 {
        kind,
        target: format!("CHG-004#{finding}"),
        reason: "A concrete disposition based on the persisted reviewer evidence.".into(),
        evidence: vec!["plan-review:snapshot-1".into()],
        expected_revision: "a".repeat(64),
        contradiction: None,
    }
}

fn available_evidence() -> std::collections::HashSet<String> {
    [
        "plan-review:snapshot-1".into(),
        "workspace-summary".into(),
        "product-spec:architecture-and-constraints".into(),
    ]
    .into()
}

fn evidence_content() -> std::collections::HashMap<String, String> {
    [(
        "product-spec:architecture-and-constraints".into(),
        "The system shall support offline operation for all approved planning actions.".into(),
    )]
    .into()
}

#[test]
fn every_review_finding_requires_one_current_typed_manager_disposition() {
    let reviews = target();
    let actions = [
        action(ManagerActionKind::RepairPlan, "F-1"),
        action(ManagerActionKind::PromoteFollowUp, "F-2"),
    ];
    let evidence = available_evidence();
    let contents = evidence_content();
    assert!(validate_dispositions(&actions, &reviews, &evidence, &contents).is_ok());
    assert!(validate_dispositions(&actions[..1], &reviews, &evidence, &contents).is_err());
    assert!(
        validate_dispositions(
            &[actions[0].clone(), actions[0].clone()],
            &reviews,
            &evidence,
            &contents
        )
        .is_err()
    );

    let mut stale = action(ManagerActionKind::AskUser, "F-1");
    stale.expected_revision = "b".repeat(64);
    assert!(validate_dispositions(&[stale], &reviews, &evidence, &contents).is_err());
}

#[test]
fn contesting_a_finding_requires_separate_evidence() {
    let reviews = target();
    let evidence = available_evidence();
    let contents = evidence_content();
    let mut contest = action(ManagerActionKind::ContestFinding, "F-1");
    assert!(validate_dispositions(&[contest.clone()], &reviews, &evidence, &contents).is_err());
    contest.evidence.push("independent-counterexample".into());
    let second = action(ManagerActionKind::RepairPlan, "F-2");
    assert!(
        validate_dispositions(
            &[contest.clone(), second.clone()],
            &reviews,
            &evidence,
            &contents
        )
        .is_err()
    );
    contest.evidence.pop();
    contest
        .evidence
        .push("product-spec:architecture-and-constraints".into());
    contest.reason = "The architecture specification requires this behavior, which directly contradicts the review finding.".into();
    contest.contradiction = Some(crate::harness::prompt_catalog::ManagerContradictionV1 {
        finding_claim: "the finding location: persisted correction".into(),
        source_evidence_id: "product-spec:architecture-and-constraints".into(),
        source_quote: "The system shall support offline operation".into(),
        explanation: "This requirement directly contradicts the review claim that offline operation is excluded.".into(),
    });
    assert!(
        validate_dispositions(
            &[contest.clone(), second.clone()],
            &reviews,
            &evidence,
            &contents
        )
        .is_ok()
    );
    contest.contradiction.as_mut().unwrap().finding_claim =
        "The plan omits offline operation.".into();
    assert!(
        validate_dispositions(
            &[contest.clone(), second.clone()],
            &reviews,
            &evidence,
            &contents
        )
        .is_err(),
        "a contest claim must bind to the exact persisted finding"
    );
    contest.contradiction.as_mut().unwrap().finding_claim =
        "the finding location: persisted correction".into();
    contest.contradiction.as_mut().unwrap().source_quote =
        "The architecture requires autonomous approval".into();
    assert!(
        validate_dispositions(
            &[contest.clone(), second.clone()],
            &reviews,
            &evidence,
            &contents
        )
        .is_err()
    );
    contest.contradiction.as_mut().unwrap().source_quote =
        "The system shall support offline operation".into();
    contest.contradiction.as_mut().unwrap().source_evidence_id = "workspace-summary".into();
    assert!(
        validate_dispositions(
            &[contest.clone(), second.clone()],
            &reviews,
            &evidence,
            &contents
        )
        .is_err()
    );
    contest.reason = "I disagree.".into();
    assert!(
        validate_dispositions(
            &[contest.clone(), second.clone()],
            &reviews,
            &evidence,
            &contents
        )
        .is_err()
    );
    contest.reason =
        "A concrete contradiction with the review finding is supported by the cited evidence."
            .into();
    contest.evidence.pop();
    contest.evidence.push("workspace-summary".into());
    assert!(validate_dispositions(&[contest, second], &reviews, &evidence, &contents).is_err());
}
