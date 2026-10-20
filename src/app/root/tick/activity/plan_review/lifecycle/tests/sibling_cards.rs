use super::*;

#[test]
fn arbitration_keeps_active_sibling_open_without_counting_its_receipt() {
    // Production boundary: the arbitration result reducer updates persisted Project work for
    // one finding among several on the same snapshot. A key-matching unit test cannot prove the
    // save path preserves neighboring cards. External dependencies: none. Setup cost: one
    // temporary Git project with two contest cards.
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-q"]);
    git(temp.path(), &["config", "user.name", "Fixture User"]);
    git(
        temp.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(temp.path().join("README.md"), "fixture\n").unwrap();
    git(temp.path(), &["add", "README.md"]);
    git(temp.path(), &["commit", "-qm", "fixture"]);
    let mut project = crate::app::session::test_support::project_from(temp.path());
    let hash = "d".repeat(64);
    let mut cards = Vec::new();
    for finding_id in ["F-1", "F-2"] {
        let mut card = Work::new(
            String::new(),
            format!("Independent arbitration: {finding_id}"),
            "Assess the contested finding".into(),
            format!("Snapshot hash: {hash}\nOriginal state for {finding_id}"),
        );
        card.kind = WorkKind::PlanReviewDisposition;
        card.status = WorkStatus::NeedsAttention;
        card.feature_id = Some("CHG-021".into());
        card.key = format!("task:manager-plan-contest:{hash}:{finding_id}");
        cards.push(card);
    }
    project.planning_work.extend(cards);
    let arbitration = crate::app::root::plan_review::ArbitrationContext {
        finding_id: "F-1".into(),
        evidence_id: "product-spec:reporting-contract".into(),
        evidence_sha256: "a".repeat(64),
        source_quote: "Empty results produce a valid file.".into(),
        source_content: "Empty results produce a valid file.".into(),
        rationale: "The product specification directly requires a valid empty export.".into(),
    };
    let verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: hash.clone(),
        examined_files: Vec::new(),
        tested_evidence_ids: Vec::new(),
        unexamined_areas: Vec::new(),
        pass: false,
        outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::BlockingFindings),
        coverage: Vec::new(),
        findings: ["F-1", "F-2"]
            .into_iter()
            .map(|id| crate::harness::prompt_catalog::ReviewFindingV1 {
                id: id.into(),
                requirement: "REQ-1".into(),
                location_detail: None,
                rationale: String::new(),
                affected_requirement_ids: Vec::new(),
                suggested_action: None,
                severity: crate::harness::prompt_catalog::Severity::High,
                kind: crate::harness::prompt_catalog::FindingKind::Requirements,
                scope: None,
                location: "requirement REQ-1".into(),
                evidence: vec!["plan:snapshot".into()],
                repro: Some("The issue remains after arbitration.".into()),
                proposed_correction: "Revise the plan to address the finding.".into(),
            })
            .collect(),
        untested: Vec::new(),
    };
    let receipt = crate::core::lifecycle::GateReceipt {
        receipt_uid: "receipt-f1".into(),
        subject_uid: "snapshot".into(),
        subject_sha256: hash,
        actor: crate::core::lifecycle::GateActor::IndependentReviewer,
        decision: crate::core::lifecycle::GateDecision::ChangesRequested,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![arbitration.evidence_id.clone()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: Some("reviewer".into()),
        author_run_id: Some("author".into()),
        reviewer_session_id: Some("review-session".into()),
        author_session_id: Some("author-session".into()),
    };
    record_plan_arbitration_work(&mut project, "CHG-021", &arbitration, &verdict, &receipt)
        .unwrap();
    assert_eq!(project.planning_work[1].status, WorkStatus::NeedsAttention);
    assert!(
        project.planning_work[1]
            .detail
            .contains("Effective arbitration receipt: receipt-f1")
    );
    assert!(
        !project.planning_work[1]
            .detail
            .contains("Independent arbitration receipt:")
    );
}

#[test]
fn arbitration_closes_siblings_removed_from_the_effective_verdict() {
    // Production boundary: persist the effective verdict across multiple Board cards and
    // confirm resolved siblings close without gaining a counted arbitration attempt. Predicate
    // tests cannot prove the Project work save path. External dependencies: none. Setup cost:
    // one temporary local Git project with two fixture cards.
    let temp = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-q"]);
    git(temp.path(), &["config", "user.name", "Fixture User"]);
    git(
        temp.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    std::fs::write(temp.path().join("README.md"), "fixture\n").unwrap();
    git(temp.path(), &["add", "README.md"]);
    git(temp.path(), &["commit", "-qm", "fixture"]);
    let mut project = crate::app::session::test_support::project_from(temp.path());
    let hash = "e".repeat(64);
    for id in ["F-1", "F-2"] {
        let mut work = Work::new(
            String::new(),
            id.into(),
            "Review finding".into(),
            format!("Snapshot hash: {hash}"),
        );
        work.kind = WorkKind::PlanReviewDisposition;
        work.status = WorkStatus::NeedsAttention;
        work.feature_id = Some("CHG-021".into());
        work.key = format!("task:manager-plan-contest:{hash}:{id}");
        project.planning_work.push(work);
    }
    let arbitration = crate::app::root::plan_review::ArbitrationContext {
        finding_id: "F-1".into(),
        evidence_id: "product-spec:reporting-contract".into(),
        evidence_sha256: "a".repeat(64),
        source_quote: "Empty results produce a valid file.".into(),
        source_content: "Empty results produce a valid file.".into(),
        rationale: "The specification requires valid empty exports.".into(),
    };
    let verdict = crate::harness::prompt_catalog::ReviewVerdictV1 {
        subject_hash: hash.clone(),
        examined_files: Vec::new(),
        tested_evidence_ids: Vec::new(),
        unexamined_areas: Vec::new(),
        pass: true,
        outcome: Some(crate::harness::prompt_catalog::ReviewOutcomeV1::Pass),
        coverage: Vec::new(),
        findings: Vec::new(),
        untested: Vec::new(),
    };
    let receipt = crate::core::lifecycle::GateReceipt {
        receipt_uid: "receipt-f1".into(),
        subject_uid: "snapshot".into(),
        subject_sha256: hash,
        actor: crate::core::lifecycle::GateActor::IndependentReviewer,
        decision: crate::core::lifecycle::GateDecision::Approved,
        recorded_at_unix_ms: 1,
        evidence_refs: vec![arbitration.evidence_id.clone()],
        prompt_sha256: None,
        prompt_manifest: None,
        reviewer_run_id: None,
        author_run_id: None,
        reviewer_session_id: None,
        author_session_id: None,
    };
    record_plan_arbitration_work(&mut project, "CHG-021", &arbitration, &verdict, &receipt)
        .unwrap();
    assert_eq!(project.planning_work[0].status, WorkStatus::Done);
    assert_eq!(project.planning_work[1].status, WorkStatus::Done);
    assert!(
        project.planning_work[1]
            .detail
            .contains("Effective arbitration receipt: receipt-f1")
    );
}
