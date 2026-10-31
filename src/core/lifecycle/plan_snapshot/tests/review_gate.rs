use super::*;

#[test]
fn task_gate_requires_a_current_independent_approved_plan_review() {
    use crate::artifacts::planning_store::{PlanningStore, StoreMode};
    use crate::core::lifecycle::GateDecision;
    let root = std::env::temp_dir().join(format!("plan-review-{}", uuid::Uuid::new_v4()));
    crate::artifacts::migration::bootstrap_product(&root, "Plan Review Fixture").unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.author_run_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    snapshot.author_session_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    persist_snapshot(&store, &snapshot);
    assert!(
        !has_approved_review(&store, &snapshot).unwrap(),
        "missing reviewer must block"
    );

    let stale = plan_receipt(&snapshot, GateDecision::Approved);
    let mut stale = stale;
    stale.subject_sha256 = "b".repeat(64);
    assert!(
        review_change(&store, &snapshot, &stale).is_err(),
        "stale plan hash must be rejected"
    );

    let mut same_author = plan_receipt(&snapshot, GateDecision::Approved);
    same_author.reviewer_run_id = same_author.author_run_id.clone();
    assert!(
        review_change(&store, &snapshot, &same_author).is_err(),
        "same author cannot review"
    );

    let mut forged_author_run = plan_receipt(&snapshot, GateDecision::Approved);
    forged_author_run.author_run_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    assert!(
        review_change(&store, &snapshot, &forged_author_run).is_err(),
        "review receipt author run must match the immutable snapshot provenance"
    );
    let mut forged_author_session = plan_receipt(&snapshot, GateDecision::Approved);
    forged_author_session.author_session_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    assert!(
        review_change(&store, &snapshot, &forged_author_session).is_err(),
        "review receipt author session must match the immutable snapshot provenance"
    );

    let premature = plan_receipt(&snapshot, GateDecision::Approved);
    assert!(
        review_change(&store, &snapshot, &premature).is_err(),
        "review must be refused until the exact plan is currently user-confirmed"
    );
    persist_user_confirmation(&store, &snapshot);

    let rejected = plan_receipt(&snapshot, GateDecision::Rejected);
    let finding_verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: snapshot.content_hash.clone(),
        examined_files: Vec::new(),
        tested_evidence_ids: Vec::new(),
        unexamined_areas: Vec::new(),
        pass: false,
        outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::BlockingFindings),
        coverage: snapshot
            .requirements
            .iter()
            .enumerate()
            .map(
                |(index, requirement)| crate::harness::prompt_catalog::CoverageV1 {
                    requirement: requirement.stable_id.clone(),
                    status: if index == 0 {
                        crate::harness::prompt_catalog::CoverageStatus::Fail
                    } else {
                        crate::harness::prompt_catalog::CoverageStatus::Pass
                    },
                    evidence: vec![format!("plan:{}", snapshot.uid)],
                },
            )
            .collect(),
        findings: vec![crate::harness::prompt_catalog::ReviewFindingV1 {
            id: "F-1".into(),
            requirement: snapshot.requirements[0].stable_id.clone(),
            location_detail: None,
            rationale: String::new(),
            affected_requirement_ids: Vec::new(),
            suggested_action: None,
            severity: crate::harness::prompt_catalog::Severity::High,
            kind: crate::harness::prompt_catalog::FindingKind::Requirements,
            scope: None,
            location: "requirement REQ-001".into(),
            evidence: vec![format!("plan:{}", snapshot.uid)],
            repro: Some("The export acceptance text omits empty results.".into()),
            proposed_correction: "Specify the empty export behavior.".into(),
        }],
        untested: Vec::new(),
    };
    persist_review(&store, &snapshot, &rejected, &finding_verdict);
    assert!(
        !has_approved_review(&store, &snapshot).unwrap(),
        "rejected review must block"
    );
    let reopened = PlanningStore::new(
        store.project_id,
        store.root.clone(),
        StoreMode::ManagedLocal,
    );
    assert_eq!(
        crate::core::lifecycle::plan_snapshot::plan_review_verdict(&reopened, &snapshot).unwrap(),
        Some(finding_verdict.clone()),
        "actionable review findings must survive store reopen"
    );
    let mut remaining_finding_verdict = finding_verdict.clone();
    let mut second_finding = finding_verdict.findings[0].clone();
    second_finding.id = "F-2".into();
    remaining_finding_verdict.findings.push(second_finding);
    let arbitrator_receipt = plan_receipt(&snapshot, GateDecision::Rejected);
    let mut uncited_receipt = arbitrator_receipt.clone();
    uncited_receipt
        .evidence_refs
        .retain(|reference| reference != "product-spec:reporting-contract");
    assert!(
        crate::core::lifecycle::plan_snapshot::arbitration_change(
            &reopened,
            &snapshot,
            "F-1",
            "product-spec:reporting-contract",
            &"a".repeat(64),
            "Empty results produce a valid file.",
            "The supplied product specification directly requires a valid export for empty selections.",
            &uncited_receipt,
            &remaining_finding_verdict,
        )
        .is_err(),
        "arbitration cannot persist content that is absent from its receipt"
    );
    let arbitration = crate::core::lifecycle::plan_snapshot::arbitration_change(
        &reopened,
        &snapshot,
        "F-1",
        "product-spec:reporting-contract",
        &"a".repeat(64),
        "Empty results produce a valid file.",
        "The supplied product specification directly requires a valid export for empty selections.",
        &arbitrator_receipt,
        &remaining_finding_verdict,
    )
    .unwrap()
    .unwrap();
    reopened
        .transaction_with_revision_and_record_revisions(
            &[(arbitration.0, arbitration.1)],
            Some(&reopened.revision().unwrap()),
            &[arbitration.2],
        )
        .unwrap();
    let second_arbitration = crate::core::lifecycle::plan_snapshot::arbitration_change(
        &reopened,
        &snapshot,
        "F-2",
        "product-spec:reporting-contract",
        &"a".repeat(64),
        "Empty results produce a valid file.",
        "The supplied product specification directly requires a valid export for empty selections.",
        &plan_receipt(&snapshot, GateDecision::Rejected),
        &finding_verdict,
    )
    .unwrap()
    .unwrap();
    reopened
        .transaction_with_revision_and_record_revisions(
            &[(second_arbitration.0, second_arbitration.1)],
            Some(&reopened.revision().unwrap()),
            &[second_arbitration.2],
        )
        .unwrap();
    let final_arbitration = crate::core::lifecycle::plan_snapshot::arbitration_change(
        &reopened,
        &snapshot,
        "F-1",
        "product-spec:reporting-contract",
        &"a".repeat(64),
        "Empty results produce a valid file.",
        "The supplied product specification directly requires a valid export for empty selections.",
        &plan_receipt(&snapshot, GateDecision::Approved),
        &passing_verdict_for(&snapshot),
    )
    .unwrap()
    .unwrap();
    reopened
        .transaction_with_revision_and_record_revisions(
            &[(final_arbitration.0, final_arbitration.1)],
            Some(&reopened.revision().unwrap()),
            &[final_arbitration.2],
        )
        .unwrap();
    assert!(
        crate::core::lifecycle::plan_snapshot::arbitration_change(
            &reopened,
            &snapshot,
            "F-1",
            "product-spec:reporting-contract",
            &"a".repeat(64),
            "Empty results produce a valid file.",
            "The supplied product specification directly requires a valid export for empty selections.",
            &plan_receipt(&snapshot, GateDecision::Approved),
            &passing_verdict_for(&snapshot),
        )
        .is_err(),
        "a third arbitration receipt for the same finding must be rejected"
    );
    assert!(has_approved_review(&reopened, &snapshot).unwrap());
    assert_eq!(
        crate::core::lifecycle::plan_snapshot::plan_review_verdict(&reopened, &snapshot).unwrap(),
        Some(passing_verdict_for(&snapshot)),
        "the durable arbitration verdict becomes the effective exact-snapshot decision"
    );

    let next_root =
        std::env::temp_dir().join(format!("plan-review-approved-{}", uuid::Uuid::new_v4()));
    crate::artifacts::migration::bootstrap_product(&next_root, "Approved Plan Review Fixture")
        .unwrap();
    let approved_store =
        PlanningStore::new(uuid::Uuid::new_v4(), &next_root, StoreMode::ManagedLocal);
    persist_snapshot(&approved_store, &snapshot);
    persist_user_confirmation(&approved_store, &snapshot);
    let approved = plan_receipt(&snapshot, GateDecision::Approved);
    let passing_verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: snapshot.content_hash.clone(),
        examined_files: Vec::new(),
        tested_evidence_ids: Vec::new(),
        unexamined_areas: Vec::new(),
        pass: true,
        outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass),
        coverage: snapshot
            .requirements
            .iter()
            .map(|requirement| crate::harness::prompt_catalog::CoverageV1 {
                requirement: requirement.stable_id.clone(),
                status: crate::harness::prompt_catalog::CoverageStatus::Pass,
                evidence: vec![format!("plan:{}", snapshot.uid)],
            })
            .collect(),
        findings: Vec::new(),
        untested: Vec::new(),
    };
    persist_review(&approved_store, &snapshot, &approved, &passing_verdict);
    assert!(has_approved_review(&approved_store, &snapshot).unwrap());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(next_root);
}

fn passing_verdict_for(
    snapshot: &PlanSnapshotV1,
) -> crate::harness::prompt_catalog::ReviewVerdictV1 {
    crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: snapshot.content_hash.clone(),
        examined_files: Vec::new(),
        tested_evidence_ids: Vec::new(),
        unexamined_areas: Vec::new(),
        pass: true,
        outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass),
        coverage: snapshot
            .requirements
            .iter()
            .map(|requirement| crate::harness::prompt_catalog::CoverageV1 {
                requirement: requirement.stable_id.clone(),
                status: crate::harness::prompt_catalog::CoverageStatus::Pass,
                evidence: vec![format!("plan:{}", snapshot.uid)],
            })
            .collect(),
        findings: Vec::new(),
        untested: Vec::new(),
    }
}
