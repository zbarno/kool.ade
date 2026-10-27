use super::*;
use crate::harness::prompt_catalog::ReviewVerdictV1;
use crate::runtime_qa::{QaFindingCategory, QaFindingSeverity};
use std::sync::{Arc, Mutex};

/// Production boundary: the review stage promotes failed driver assertions
/// into typed, report-bound QaFindingV1 entries that reviewers and operators
/// consume. Invariant: every promoted finding keeps category, severity,
/// blocking flag, code/requirement bindings, reproducible-step, and
/// screenshot-reference integrity, and only Failed assertions are promoted.
/// A rootless driver/container test cannot isolate this mapping from driver
/// timing; this test drives the real orchestrator path with a failing fake
/// driver and inspects the real generated manifest at the review seam. Setup
/// cost is a temporary local store, no external services.
#[test]
fn seeded_defect_failures_promote_typed_blocking_findings_with_intact_bindings() {
    let (request, scenarios, lease) = fixture();
    let captured = Arc::new(Mutex::new(None));
    let review_capture = captured.clone();
    let mut driver = FakeDriver {
        calls: 0,
        assertion_status: QaAssertionStatus::Failed,
        failure_before_actions: false,
        failure_on_call: None,
    };
    let temp = tempfile::tempdir().unwrap();
    let store = RuntimeQaStore::new(temp.path().join("qa"));
    let mut reviewer = |evidence: &QaEvidenceManifestV1,
                        candidate: &QaResultV1,
                        screenshots: &[QaScreenshotArtifactV1]| {
        *review_capture.lock().unwrap() = Some(evidence.clone());
        Ok(ReviewVerdictV1 {
            subject_hash: crate::runtime_qa::qa_review_subject_sha256(
                &request, candidate, evidence,
            )?,
            examined_files: Vec::new(),
            tested_evidence_ids: screenshots
                .iter()
                .map(|screenshot| screenshot.id.clone())
                .collect(),
            unexamined_areas: Vec::new(),
            pass: false,
            outcome: None,
            coverage: Vec::new(),
            findings: Vec::new(),
            untested: Vec::new(),
        })
    };
    let reviewer_run = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = reviewer_prompt(&request, &reviewer_run, &reviewer_session);
    RuntimeQaOrchestrator::new(&store)
        .execute_reviewed(
            QaExecutionContext {
                request: &request,
                scenarios: &scenarios,
                authorization: QaTargetAuthorization::Web(&lease),
                driver: &mut driver,
                reviewer_execution_id: &reviewer_run,
                reviewer_session_id: &reviewer_session,
                prompt,
            },
            &mut reviewer,
        )
        .unwrap();

    let mut evidence =
        captured.lock().unwrap().take().expect(
            "a failing driver run must still reach the review seam with generated evidence",
        );
    let failed_count = evidence
        .assertions
        .iter()
        .filter(|assertion| assertion.status == QaAssertionStatus::Failed)
        .count();
    assert!(
        failed_count >= 1,
        "fake driver must produce failed assertions"
    );

    // Seed-class semantics the findings must carry through verbatim: clipped
    // modal, overlapping controls, invisible text, wrong interaction target,
    // surfaced JS error, and failing save/reload.
    let seeded_classes = [
        (
            "The dialog is fully visible inside the viewport",
            "Dialog clipped 12px at the bottom edge",
        ),
        (
            "Primary and secondary buttons occupy disjoint regions",
            "Primary button overlaps the secondary button",
        ),
        (
            "Status label is legible against the background",
            "Label rendered with indistinguishable contrast",
        ),
        (
            "Opening details navigates to the details pane",
            "Click dismissed the dialog instead of opening details",
        ),
        (
            "Save completes without console errors",
            "Console error: seeded fixture exception",
        ),
        (
            "Saved row persists across a reload",
            "Row vanished after reload",
        ),
    ];
    for (index, assertion) in evidence.assertions.iter_mut().enumerate() {
        if assertion.status == QaAssertionStatus::Failed {
            let (expected, observed) = seeded_classes[index % seeded_classes.len()];
            assertion.expected = expected.into();
            assertion.observed = observed.into();
        }
    }

    let findings = review::failed_assertion_findings(&request, &scenarios, &evidence);
    let failed_positions: Vec<usize> = evidence
        .assertions
        .iter()
        .enumerate()
        .filter(|(_, assertion)| assertion.status == QaAssertionStatus::Failed)
        .map(|(index, _)| index)
        .collect();
    assert_eq!(findings.len(), failed_count);

    for (position, finding) in findings.iter().enumerate() {
        let source_index = failed_positions[position];
        let source = &evidence.assertions[source_index];
        let (expected, observed) = seeded_classes[source_index % seeded_classes.len()];
        assert_eq!(finding.category, QaFindingCategory::Functional);
        assert_eq!(finding.severity, QaFindingSeverity::High);
        assert!(
            finding.blocking,
            "seeded acceptance failures block the QA gate"
        );
        assert_eq!(finding.code_sha, request.code_head_sha);
        assert_eq!(finding.scenario_uid, source.scenario_ids[0]);
        assert_eq!(finding.requirement_uids, source.requirement_ids);
        assert_eq!(finding.expected, expected);
        assert_eq!(finding.actual, observed);
        let scenario = scenarios
            .iter()
            .find(|item| item.id == finding.scenario_uid)
            .expect("finding must reference an executed scenario");
        assert_eq!(
            finding.reproducible_steps.len(),
            scenario.steps.len(),
            "reproducible steps mirror the scenario step list"
        );
        assert!(finding.reproducible_steps[0].starts_with("Step 1: "));
        let scenario_shots: Vec<String> = evidence
            .screenshots
            .iter()
            .filter(|shot| shot.scenario_id == scenario.id)
            .map(|shot| shot.id.clone())
            .collect();
        assert_eq!(
            finding.screenshot_ids.len(),
            scenario_shots.len(),
            "every scenario screenshot anchors the finding"
        );
        assert!(
            finding
                .screenshot_ids
                .iter()
                .all(|id| scenario_shots.iter().any(|known| known == id))
        );
    }

    // Negative filter: only Failed assertions promote findings. Passed and
    // Untested evidence must contribute none, regardless of its text.
    let mut passed_clone = evidence.clone();
    for assertion in passed_clone.assertions.iter_mut() {
        assertion.status = QaAssertionStatus::Passed;
    }
    assert!(review::failed_assertion_findings(&request, &scenarios, &passed_clone).is_empty());
    let mut untested_clone = evidence.clone();
    for assertion in untested_clone.assertions.iter_mut() {
        assertion.status = QaAssertionStatus::Untested;
    }
    assert!(review::failed_assertion_findings(&request, &scenarios, &untested_clone).is_empty());
}
