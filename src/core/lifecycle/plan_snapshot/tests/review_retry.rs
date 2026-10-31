use super::*;

#[test]
fn review_unable_attempts_are_persisted_revision_checked_and_bounded() {
    use crate::artifacts::planning_store::{PlanningStore, StoreError, StoreMode};
    use crate::core::lifecycle::GateDecision;
    let root = std::env::temp_dir().join(format!("plan-review-retry-{}", uuid::Uuid::new_v4()));
    crate::artifacts::migration::bootstrap_product(&root, "Plan Review Retry Fixture").unwrap();
    let mut store = PlanningStore::new(uuid::Uuid::new_v4(), &root, StoreMode::ManagedLocal);
    let (identity, markdown) = identified(&spec());
    let contract = crate::core::workflow::feature_contract(&markdown);
    let mut snapshot =
        PlanSnapshotV1::from_feature("CHG-021", &identity, &markdown, &contract).unwrap();
    snapshot.author_run_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    snapshot.author_session_id = Some(uuid::Uuid::new_v4().hyphenated().to_string());
    persist_snapshot(&store, &snapshot);
    persist_user_confirmation(&store, &snapshot);

    let mut persisted_verdict = None;
    for attempt in 0..3 {
        let receipt = plan_receipt(&snapshot, GateDecision::ChangesRequested);
        let verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
            subject_hash: snapshot.content_hash.clone(),
            examined_files: Vec::new(),
            tested_evidence_ids: Vec::new(),
            unexamined_areas: Vec::new(),
            pass: false,
            outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable),
            coverage: snapshot
                .requirements
                .iter()
                .map(|requirement| crate::harness::prompt_catalog::CoverageV1 {
                    requirement: requirement.stable_id.clone(),
                    status: crate::harness::prompt_catalog::CoverageStatus::Untested,
                    evidence: vec![format!("plan:{}", snapshot.uid)],
                })
                .collect(),
            findings: Vec::new(),
            untested: vec![
                "The frozen plan lacks sufficient evidence to assess deployment.".into(),
            ],
        };
        let (path, bytes, check) =
            review_change_with_verdict(&store, &snapshot, &receipt, &verdict)
                .unwrap()
                .expect("each independent retry has a new receipt");
        store
            .transaction_with_record_revisions(&[(path, bytes)], &[check])
            .unwrap();
        persisted_verdict = Some(verdict.clone());
        if attempt == 0 {
            store = PlanningStore::new(
                store.project_id,
                store.root.clone(),
                StoreMode::ManagedLocal,
            );
        }
        assert_eq!(
            crate::core::lifecycle::plan_snapshot::review_retry_available(&store, &snapshot)
                .unwrap(),
            attempt < 2
        );
    }

    let receipt = plan_receipt(&snapshot, GateDecision::ChangesRequested);
    let verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: snapshot.content_hash.clone(),
        examined_files: Vec::new(),
        tested_evidence_ids: Vec::new(),
        unexamined_areas: Vec::new(),
        pass: false,
        outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable),
        coverage: snapshot
            .requirements
            .iter()
            .map(|requirement| crate::harness::prompt_catalog::CoverageV1 {
                requirement: requirement.stable_id.clone(),
                status: crate::harness::prompt_catalog::CoverageStatus::Untested,
                evidence: vec![format!("plan:{}", snapshot.uid)],
            })
            .collect(),
        findings: Vec::new(),
        untested: vec!["Review retry limit reached.".into()],
    };
    assert_eq!(
        crate::core::lifecycle::plan_snapshot::plan_review_verdict(&store, &snapshot).unwrap(),
        persisted_verdict,
        "the last ReviewUnable result remains available after reopening the store"
    );
    assert!(matches!(
        review_change_with_verdict(&store, &snapshot, &receipt, &verdict),
        Err(StoreError::MalformedState(message)) if message.contains("attempts are exhausted")
    ));
    let _ = std::fs::remove_dir_all(root);
}
