use super::*;
#[path = "arbitration_worker/harness.rs"]
mod harness_fixture;
use harness_fixture::harness;

#[test]
// Production boundary: Manager output is rendered and validated, persisted as a contest card,
// selected by the normal arbitration queue, executed by the real asynchronous review worker,
// and saved as a durable PlanReviewRecord arbitration receipt.
// Invariant: only the independent verdict for the exact confirmed snapshot becomes the effective
// gate decision; catalog or storage unit tests cannot prove worker prompt construction, fresh
// review execution, and receipt persistence together.
// External dependencies: none. Setup cost: one temporary Git project and two deterministic
// in-process model responses.
fn manager_contest_runs_independent_arbitration_and_persists_effective_receipt() {
    use crate::harness::prompt_catalog::{
        CoverageStatus, CoverageV1, ManagerActionKind, ManagerActionV1, ManagerContradictionV1,
        ManagerOutputV2, ReviewOutcomeV1, ReviewVerdictV1,
    };

    let _shield = crate::core::gitops::test_support::shield("manager-contest-arbitration-worker");
    let (mut app, root, _) = setup();
    record_test_plan_author(&mut app, "CHG-004");
    app.approve_feature_only("CHG-004");
    record_test_plan_review(
        &mut app,
        "CHG-004",
        crate::core::lifecycle::GateDecision::ChangesRequested,
    );

    let (snapshot, review_id, source_id, source_quote) = {
        let Screen::Connected(project) = &app.screen else {
            panic!("project remains connected")
        };
        let (_, markdown) = project
            .state
            .active_features
            .iter()
            .find(|(id, _)| id == "CHG-004")
            .unwrap();
        let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
            .unwrap()
            .unwrap();
        let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
            &project.state.planning_store,
            "CHG-004",
            &identity,
            markdown,
            &crate::core::workflow::feature_contract(markdown),
        )
        .unwrap();
        let evidence =
            crate::app::root::plan_review::evidence::collect(project, &snapshot).unwrap();
        let source = evidence
            .items
            .iter()
            .find(|item| item.id == "product-spec:current-capabilities")
            .expect("review evidence includes the current-capabilities product specification");
        assert!(source.id.starts_with("product-spec:"));
        let mut lines = source
            .content
            .lines()
            .filter(|line| !line.trim().is_empty());
        let first_line = lines
            .next()
            .expect("product specification has a first line");
        let second_line = lines
            .next()
            .expect("product specification has a second line");
        let start = source.content.find(first_line).unwrap();
        let second_start = start
            + first_line.len()
            + source.content[start + first_line.len()..]
                .find(second_line)
                .unwrap();
        let quote = source.content[start..second_start + second_line.len()].to_owned();
        assert_eq!(
            quote.lines().filter(|line| !line.trim().is_empty()).count(),
            2,
            "fixture exercises a quote spanning two product-spec lines"
        );
        assert!(quote.trim().len() >= 8);
        (
            snapshot.clone(),
            format!("plan-review:{}", snapshot.uid),
            source.id.clone(),
            quote,
        )
    };
    let finding_id = "TEST-FINDING".to_owned();
    let action = ManagerActionV1 {
        kind: ManagerActionKind::ContestFinding,
        target: format!("CHG-004#{finding_id}"),
        reason: "The independent finding may conflict with the cited product specification.".into(),
        evidence: vec![review_id.clone(), source_id.clone()],
        expected_revision: snapshot.content_hash.clone(),
        contradiction: Some(ManagerContradictionV1 {
            finding_claim: "test fixture requirement: Clarify the missing requirement detail.".into(),
            source_evidence_id: source_id.clone(),
            source_quote: source_quote.clone(),
            explanation: "The cited specification text appears to establish the behavior the finding says is absent.".into(),
        }),
    };
    let manager_response = serde_json::to_string(&ManagerOutputV2 {
        summary: "Request independent arbitration for the cited finding.".into(),
        proposed_actions: vec![action],
        opportunities: Vec::new(),
    })
    .unwrap();
    let arbitrator_verdict = ReviewVerdictV1 {
        subject_hash: snapshot.content_hash.clone(),
        pass: true,
        outcome: Some(ReviewOutcomeV1::Pass),
        coverage: snapshot
            .requirements
            .iter()
            .map(|requirement| CoverageV1 {
                requirement: requirement.stable_id.clone(),
                status: CoverageStatus::Pass,
                evidence: vec![format!("plan:{}", snapshot.uid)],
            })
            .collect(),
        findings: Vec::new(),
        untested: Vec::new(),
    };
    let arbitration_response = serde_json::to_string(&arbitrator_verdict).unwrap();
    let manager_replies = Arc::new(Mutex::new(std::collections::VecDeque::from([
        manager_response,
    ])));
    let arbitration_replies = Arc::new(Mutex::new(std::collections::VecDeque::from([
        arbitration_response,
    ])));
    let prompts = Arc::new(Mutex::new(Vec::new()));

    let manager = {
        let Screen::Connected(project) = &app.screen else {
            panic!("project remains connected")
        };
        crate::app::manager::Manager::start(
            project,
            &["independent plan review requested changes".into()],
            harness(manager_replies, prompts.clone()),
        )
    };
    let started = Instant::now();
    let output = loop {
        if let Some(result) = manager.result() {
            break result.expect("Manager response validates");
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "Manager did not finish"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("project remains connected")
        };
        assert!(
            crate::app::manager::persist_plan_dispositions(
                project,
                &output.proposed_actions,
                &output.evidence_hashes,
                &output.planning_revision,
            )
            .unwrap()
            .is_empty()
        );
        let (claimed_feature, arbitration) = crate::app::manager::next_plan_arbitration(project)
            .expect("persisted Manager contest is selectable");
        assert_eq!(claimed_feature, "CHG-004");
        assert_eq!(arbitration.finding_id, finding_id);
        assert_eq!(arbitration.evidence_id, source_id);

        let worker = crate::app::root::plan_review::PlanReview::start_arbitration(
            project,
            claimed_feature.clone(),
            arbitration.clone(),
            harness(arbitration_replies, prompts.clone()),
        )
        .expect("real arbitration worker prepares the current evidence and snapshot");
        let started = Instant::now();
        let outcome = loop {
            if let Some(result) = worker.take_result() {
                break result.expect("arbitration worker returns its validated verdict");
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "arbitration did not finish"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        assert!(outcome.verdict.pass);
        assert!(outcome.arbitration.is_some());
        assert_eq!(outcome.snapshot_hash, snapshot.content_hash);
        let arbitration = outcome.arbitration.unwrap();
        let reviewer = crate::core::lifecycle::GateReceipt {
            receipt_uid: uuid::Uuid::new_v4().hyphenated().to_string(),
            subject_uid: outcome.snapshot_uid.clone(),
            subject_sha256: outcome.snapshot_hash.clone(),
            actor: crate::core::lifecycle::GateActor::IndependentReviewer,
            decision: crate::core::lifecycle::GateDecision::Approved,
            recorded_at_unix_ms: 1,
            evidence_refs: vec![
                format!("plan:{}", outcome.snapshot_uid),
                arbitration.evidence_id.clone(),
            ],
            prompt_sha256: Some(outcome.prompt.identity_sha256()),
            prompt_manifest: Some(outcome.prompt),
            reviewer_run_id: Some(outcome.reviewer_run_id),
            author_run_id: Some(outcome.author_run_id),
            reviewer_session_id: Some(outcome.reviewer_session_id),
            author_session_id: Some(outcome.author_session_id),
        };
        crate::core::workflow::record_plan_arbitration_if_current(
            &project.state.planning_store,
            &claimed_feature,
            &arbitration.finding_id,
            &arbitration.evidence_id,
            &arbitration.evidence_sha256,
            &arbitration.source_content,
            &arbitration.source_quote,
            &arbitration.rationale,
            &reviewer,
            &outcome.verdict,
            &project.state.baseline_planning_revision,
        )
        .unwrap();
        let effective = crate::core::lifecycle::plan_snapshot::plan_review_verdict(
            &project.state.planning_store,
            &snapshot,
        )
        .unwrap()
        .unwrap();
        assert!(effective.pass);
        assert!(effective.findings.is_empty());
        assert!(
            crate::core::lifecycle::plan_snapshot::has_approved_review(
                &project.state.planning_store,
                &snapshot,
            )
            .unwrap()
        );
        let receipt = crate::core::lifecycle::plan_snapshot::plan_review_receipt(
            &project.state.planning_store,
            &snapshot,
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            receipt.decision,
            crate::core::lifecycle::GateDecision::Approved
        );
        assert_ne!(receipt.receipt_uid, "");
    }
    let prompts = prompts.lock().unwrap();
    assert_eq!(prompts.len(), 2);
    assert!(prompts[0].contains(&review_id));
    assert!(prompts[1].contains("Independent arbitration request"));
    let encoded_quote = serde_json::to_string(&source_quote).unwrap();
    let escaped_quote_contents = &encoded_quote[1..encoded_quote.len() - 1];
    assert!(prompts[1].contains(escaped_quote_contents));
    drop(prompts);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
