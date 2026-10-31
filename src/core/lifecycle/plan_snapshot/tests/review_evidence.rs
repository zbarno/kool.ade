use super::*;
use crate::artifacts::planning_store::{PlanningStore, StoreMode};
use crate::core::lifecycle::{GateDecision, PlanSnapshotV1};
use crate::harness::prompt_catalog::{
    CoverageStatus, CoverageV1, FindingKind, ReviewFindingV1, ReviewOutcomeV1, ReviewVerdictV1,
    Severity,
};

fn setup_review() -> (tempfile::TempDir, PlanningStore, PlanSnapshotV1) {
    let root = tempfile::tempdir().unwrap();
    crate::artifacts::migration::bootstrap_product(root.path(), "Review Evidence Fixture").unwrap();
    let store = PlanningStore::new(uuid::Uuid::new_v4(), root.path(), StoreMode::ManagedLocal);
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.author_run_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    snapshot.author_session_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    persist_snapshot(&store, &snapshot);
    persist_user_confirmation(&store, &snapshot);
    (root, store, snapshot)
}

fn blocking_verdict(snapshot: &PlanSnapshotV1) -> ReviewVerdictV1 {
    let evidence = format!("plan:{}", snapshot.uid);
    ReviewVerdictV1 {
        subject_hash: snapshot.content_hash.clone(),
        examined_files: Vec::new(),
        tested_evidence_ids: Vec::new(),
        unexamined_areas: Vec::new(),
        pass: false,
        outcome: Some(ReviewOutcomeV1::BlockingFindings),
        coverage: snapshot
            .requirements
            .iter()
            .enumerate()
            .map(|(index, requirement)| CoverageV1 {
                requirement: requirement.stable_id.clone(),
                status: if index == 0 {
                    CoverageStatus::Fail
                } else {
                    CoverageStatus::Pass
                },
                evidence: vec![evidence.clone()],
            })
            .collect(),
        findings: vec![ReviewFindingV1 {
            id: "F-EVIDENCE".into(),
            requirement: snapshot.requirements[0].stable_id.clone(),
            location_detail: None,
            rationale: String::new(),
            affected_requirement_ids: Vec::new(),
            suggested_action: None,
            severity: Severity::High,
            kind: FindingKind::Requirements,
            scope: None,
            location: "requirement evidence binding".into(),
            evidence: vec![evidence],
            repro: Some("The finding cites the exact plan snapshot.".into()),
            proposed_correction: "Bind the saved evidence references to the receipt.".into(),
        }],
        untested: Vec::new(),
    }
}

// Production boundary: durable plan-review write and reopen validation. A validator-only unit
// test cannot prove both record creation and record loading enforce receipt evidence membership.
// Invariant: every finding and coverage reference is cited by its associated review receipt.
// External dependencies: none. Setup cost: temporary local planning stores with no model calls.
#[test]
fn review_evidence_must_be_receipt_bound_on_write_and_read() {
    for target in ["finding", "coverage"] {
        let (_root, store, snapshot) = setup_review();
        let receipt = plan_receipt(&snapshot, GateDecision::Rejected);
        let mut verdict = blocking_verdict(&snapshot);
        if target == "finding" {
            verdict.findings[0].evidence[0] = "uncited:fabricated-source".into();
        } else {
            verdict.coverage[0].evidence[0] = "uncited:fabricated-source".into();
        }
        assert!(
            crate::core::lifecycle::plan_snapshot::review_change_with_verdict(
                &store, &snapshot, &receipt, &verdict,
            )
            .is_err(),
            "uncited {target} evidence must be rejected before persistence"
        );
    }

    for target in ["finding", "coverage"] {
        let (_root, store, snapshot) = setup_review();
        let receipt = plan_receipt(&snapshot, GateDecision::Rejected);
        let verdict = blocking_verdict(&snapshot);
        persist_review(&store, &snapshot, &receipt, &verdict);
        let path = store.root.join(format!(
            "state/lifecycle_plan_reviews/{}.json",
            snapshot.uid
        ));
        let mut record: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let evidence = if target == "finding" {
            &mut record["verdict"]["findings"][0]["evidence"][0]
        } else {
            &mut record["verdict"]["coverage"][0]["evidence"][0]
        };
        *evidence = serde_json::Value::String("uncited:fabricated-source".into());
        std::fs::write(&path, serde_json::to_vec_pretty(&record).unwrap()).unwrap();
        assert!(
            crate::core::lifecycle::plan_snapshot::plan_review_verdict(&store, &snapshot).is_err(),
            "uncited {target} evidence must be rejected after loading the durable record"
        );
    }
}
