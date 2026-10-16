use super::*;
#[path = "traceability/support.rs"]
pub(crate) mod support;
use support::{answered_decision, reviewed_feature};

// Production boundary: file-backed LifecycleStore append, aggregate, and both
// UI-facing trace queries. Unit validators cannot prove persisted reference
// resolution or revision-exact reverse lookup; uses only temp Git/filesystem
// fixtures and has moderate setup cost, with no network or service dependency.
#[test]
fn persisted_requirement_test_trace_supports_forward_and_reverse_queries() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let (feature_uid, _) = reviewed_feature(&fixture, &store, "CHG-101", "CHG-101-trace");
    let lifecycle = LifecycleStore::new(&store);
    let aggregate_before = lifecycle.product_specification_aggregate().unwrap();
    let (other_feature_uid, _) = reviewed_feature(&fixture, &store, "CHG-102", "CHG-102-trace");
    let aggregate_after = lifecycle.product_specification_aggregate().unwrap();
    assert_ne!(aggregate_before, aggregate_after);
    assert_eq!(
        aggregate_after,
        lifecycle.product_specification_aggregate().unwrap(),
        "the aggregate is deterministic across an unchanged store"
    );
    let questions = crate::core::planning::QuestionStore::new(&store);
    let (mut other_readiness, other_revision) = questions
        .load_readiness(&other_feature_uid)
        .unwrap()
        .unwrap();
    other_readiness.acceptance_tests[0].test_ref = "tests/other-feature.rs".into();
    other_readiness.revision += 1;
    questions
        .save_readiness(&other_readiness, other_revision)
        .unwrap();
    let (mut readiness, readiness_revision) =
        questions.load_readiness(&feature_uid).unwrap().unwrap();
    readiness.acceptance_tests[1].test_ref = "tests/second-requirement.rs".into();
    readiness.revision += 1;
    readiness
        .acceptance_tests
        .push(crate::core::planning::AcceptanceTestTrace {
            requirement_id: readiness.acceptance_tests[0].requirement_id.clone(),
            test_ref: "tests/feature-second-revision.rs".into(),
        });
    questions
        .save_readiness(&readiness, readiness_revision)
        .unwrap();

    let feature = lifecycle.load_feature(&feature_uid).unwrap();
    let requirement_id = crate::core::lifecycle::plan_snapshot::load_persisted(
        &store,
        feature.plan_snapshot_uid.as_deref().unwrap(),
    )
    .unwrap()
    .unwrap()
    .requirements[0]
        .stable_id
        .clone();
    let trigger = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "trace-positive")
        .unwrap();
    let content_hash = crate::core::lifecycle::product_spec::current_product_hash(&store).unwrap();
    let mut draft = Fixture::draft(&store, feature.revision, trigger.uid.clone());
    draft.content_hash = content_hash;
    draft.decision_refs = feature.decision_refs.clone();
    draft.approved_target_hash = feature.approved_plan_sha256.clone();
    draft.reconciliation_status = SpecReconciliationStatusV1::OutstandingNonblockingFindings;
    let requirements = crate::core::lifecycle::plan_snapshot::load_persisted(
        &store,
        feature.plan_snapshot_uid.as_deref().unwrap(),
    )
    .unwrap()
    .unwrap()
    .requirements;
    let first_decision = answered_decision(
        &store,
        &feature_uid,
        &requirements[0].stable_id,
        "primary test decision",
    );
    let second_decision = answered_decision(
        &store,
        &feature_uid,
        &requirements[1].stable_id,
        "independent test decision",
    );
    assert_ne!(first_decision, second_decision);
    draft.deltas.push(SpecDeltaRecordV1 {
        requirement_id: requirement_id.clone(),
        event_id: trigger.uid.clone(),
        old_target: Some("Old wording".into()),
        proposed_target: Some("Improved wording".into()),
        observed_current: None,
        classification: crate::core::lifecycle::SpecDeltaClassificationV1::Documentation,
        evidence_refs: vec![format!("spec-trigger:{}", trigger.uid)],
        decision_refs: vec![first_decision.clone()],
        test_refs: vec!["tests/feature.rs".into()],
        affected_task_ids: Vec::new(),
        needs_user_reapproval: false,
        followup_ids: Vec::new(),
        operator_decision: None,
    });
    draft.deltas.push(SpecDeltaRecordV1 {
        requirement_id: requirements[1].stable_id.clone(),
        event_id: trigger.uid.clone(),
        old_target: Some("Old independent wording".into()),
        proposed_target: Some("New independent wording".into()),
        observed_current: None,
        classification: crate::core::lifecycle::SpecDeltaClassificationV1::Documentation,
        evidence_refs: vec![format!("spec-trigger:{}", trigger.uid)],
        decision_refs: vec![second_decision.clone()],
        test_refs: vec!["tests/second-requirement.rs".into()],
        affected_task_ids: Vec::new(),
        needs_user_reapproval: false,
        followup_ids: Vec::new(),
        operator_decision: None,
    });
    let expected_sequence = lifecycle
        .load_spec_revision(&feature_uid)
        .unwrap()
        .map_or(0, |revision| revision.seq);
    lifecycle
        .append_spec_revision(&feature_uid, expected_sequence, draft)
        .unwrap();

    let forward = lifecycle
        .specification_requirement_trace(&feature_uid, &requirement_id)
        .unwrap();
    assert_eq!(forward.len(), 1);
    assert_eq!(forward[0].test_refs, ["tests/feature.rs"]);
    assert_eq!(
        forward[0].decision_refs,
        std::slice::from_ref(&first_decision)
    );
    let second_trace = lifecycle
        .specification_requirement_trace(&feature_uid, &requirements[1].stable_id)
        .unwrap();
    assert_eq!(second_trace[0].test_refs, ["tests/second-requirement.rs"]);
    assert_eq!(
        second_trace[0].decision_refs,
        std::slice::from_ref(&second_decision)
    );
    assert_eq!(
        lifecycle
            .requirements_for_specification_reference(&feature_uid, "tests/feature.rs")
            .unwrap()[0]
            .requirement_id,
        requirement_id
    );
    assert!(
        lifecycle
            .requirements_for_specification_reference(&feature_uid, "tests/unknown.rs")
            .unwrap()
            .is_empty()
    );

    let second_trigger = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "trace-second-revision")
        .unwrap();
    let mut second_draft = Fixture::draft(&store, feature.revision, second_trigger.uid.clone());
    second_draft.decision_refs = feature.decision_refs.clone();
    second_draft.approved_target_hash = feature.approved_plan_sha256.clone();
    second_draft.reconciliation_status = SpecReconciliationStatusV1::OutstandingNonblockingFindings;
    second_draft.deltas.push(SpecDeltaRecordV1 {
        requirement_id: requirement_id.clone(),
        event_id: second_trigger.uid.clone(),
        old_target: Some("Improved wording".into()),
        proposed_target: Some("Further improved wording".into()),
        observed_current: None,
        classification: crate::core::lifecycle::SpecDeltaClassificationV1::Documentation,
        evidence_refs: vec![format!("spec-trigger:{}", second_trigger.uid)],
        decision_refs: vec![first_decision],
        test_refs: vec!["tests/feature-second-revision.rs".into()],
        affected_task_ids: Vec::new(),
        needs_user_reapproval: false,
        followup_ids: Vec::new(),
        operator_decision: None,
    });
    lifecycle
        .append_spec_revision(&feature_uid, 2, second_draft)
        .unwrap();
    assert_eq!(
        lifecycle
            .specification_requirement_trace(&feature_uid, &requirement_id)
            .unwrap()
            .len(),
        2
    );
    let first_test_match = lifecycle
        .requirements_for_specification_reference(&feature_uid, "tests/feature.rs")
        .unwrap();
    assert_eq!(first_test_match.len(), 1);
    assert_eq!(first_test_match[0].revision, 2);
    let second_test_match = lifecycle
        .requirements_for_specification_reference(&feature_uid, "tests/feature-second-revision.rs")
        .unwrap();
    assert_eq!(second_test_match.len(), 1);
    assert_eq!(second_test_match[0].revision, 3);
    let first_revision_match = lifecycle
        .requirements_for_specification_reference(
            &feature_uid,
            &format!("spec-trigger:{}", trigger.uid),
        )
        .unwrap();
    assert_eq!(first_revision_match.len(), 2); // Two requirements in revision one only.
    assert!(
        first_revision_match
            .iter()
            .all(|trace| trace.revision == expected_sequence + 1)
    );
    let second_revision_match = lifecycle
        .requirements_for_specification_reference(
            &feature_uid,
            &format!("spec-trigger:{}", second_trigger.uid),
        )
        .unwrap();
    assert_eq!(second_revision_match.len(), 1);
    assert_eq!(second_revision_match[0].revision, 3);
    assert_eq!(
        lifecycle
            .product_specification_aggregate()
            .unwrap()
            .findings
            .len(),
        3,
        "separate unresolved deltas remain visible across revisions"
    );

    for (key, forged_test_ref) in [
        ("trace-unknown", "tests/unknown.rs"),
        ("trace-cross-requirement", "tests/second-requirement.rs"),
        ("trace-cross-feature", "tests/other-feature.rs"),
    ] {
        let trigger = lifecycle
            .enqueue_manual_spec_refresh(&feature_uid, key)
            .unwrap();
        let mut forged = Fixture::draft(&store, feature.revision, trigger.uid.clone());
        forged.decision_refs = feature.decision_refs.clone();
        forged.approved_target_hash = feature.approved_plan_sha256.clone();
        forged.reconciliation_status = SpecReconciliationStatusV1::OutstandingNonblockingFindings;
        forged.deltas.push(SpecDeltaRecordV1 {
            requirement_id: requirement_id.clone(),
            event_id: trigger.uid.clone(),
            old_target: Some("Old wording".into()),
            proposed_target: Some("Forged wording".into()),
            observed_current: None,
            classification: crate::core::lifecycle::SpecDeltaClassificationV1::Documentation,
            evidence_refs: vec![format!("spec-trigger:{}", trigger.uid)],
            decision_refs: Vec::new(),
            test_refs: vec![forged_test_ref.into()],
            affected_task_ids: Vec::new(),
            needs_user_reapproval: false,
            followup_ids: Vec::new(),
            operator_decision: None,
        });
        assert!(
            lifecycle
                .append_spec_revision(&feature_uid, 3, forged)
                .is_err()
        );
    }
    let decision_trigger = lifecycle
        .enqueue_manual_spec_refresh(&feature_uid, "trace-cross-decision")
        .unwrap();
    let mut forged_decision =
        Fixture::draft(&store, feature.revision, decision_trigger.uid.clone());
    forged_decision.decision_refs = feature.decision_refs.clone();
    forged_decision.approved_target_hash = feature.approved_plan_sha256.clone();
    forged_decision.reconciliation_status =
        SpecReconciliationStatusV1::OutstandingNonblockingFindings;
    forged_decision.deltas.push(SpecDeltaRecordV1 {
        requirement_id: requirement_id.clone(),
        event_id: decision_trigger.uid.clone(),
        old_target: Some("Old wording".into()),
        proposed_target: Some("Forged decision cross-link".into()),
        observed_current: None,
        classification: crate::core::lifecycle::SpecDeltaClassificationV1::Documentation,
        evidence_refs: vec![format!("spec-trigger:{}", decision_trigger.uid)],
        decision_refs: vec![second_decision],
        test_refs: vec!["tests/feature.rs".into()],
        affected_task_ids: Vec::new(),
        needs_user_reapproval: false,
        followup_ids: Vec::new(),
        operator_decision: None,
    });
    assert!(
        lifecycle
            .append_spec_revision(&feature_uid, 3, forged_decision)
            .is_err()
    );
}
