use crate::app::root::plan_review::verdict::{sanitize_verdict, unable_verdict};

#[test]
fn reviewer_panic_is_converted_to_a_retryable_execution_error() {
    let result = crate::app::root::plan_review::verdict::catch_execution_panic::<()>(|| {
        panic!("reviewer crashed")
    });
    assert_eq!(
        result.unwrap_err(),
        "independent reviewer execution panicked"
    );
}

#[test]
fn disconnected_review_worker_surfaces_an_error_instead_of_staying_busy() {
    let (sender, receiver) = std::sync::mpsc::channel();
    drop(sender);
    let review = super::PlanReview {
        result: receiver,
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let Some(Err(error)) = review.take_result() else {
        panic!("disconnected worker must surface an error")
    };
    assert_eq!(error, "independent plan review worker ended unexpectedly");
}

#[test]
fn reviewer_failure_becomes_a_complete_sanitized_review_unable_verdict() {
    let verdict = unable_verdict(
        &"a".repeat(64),
        &["REQ-001".into(), "REQ-002".into()],
        "plan:snapshot-1",
    );
    assert_eq!(
        verdict.outcome,
        Some(crate::harness::prompt_catalog::ReviewOutcomeV1::ReviewUnable)
    );
    assert!(!verdict.pass);
    assert!(verdict.findings.is_empty());
    assert_eq!(
        verdict
            .coverage
            .iter()
            .map(|coverage| coverage.requirement.as_str())
            .collect::<Vec<_>>(),
        ["REQ-001", "REQ-002"]
    );
    assert!(verdict.coverage.iter().all(|coverage| {
        matches!(
            coverage.status,
            crate::harness::prompt_catalog::CoverageStatus::Untested
        ) && coverage.evidence == ["plan:snapshot-1"]
    }));
    assert!(
        verdict
            .untested
            .iter()
            .all(|limitation| !limitation.contains("/home/") && !limitation.contains("/mnt/"))
    );
}

#[test]
fn persisted_finding_text_is_bounded_and_local_paths_are_redacted() {
    let mut verdict = unable_verdict(&"b".repeat(64), &["REQ-001".into()], "plan:snapshot-2");
    verdict
        .findings
        .push(crate::harness::prompt_catalog::ReviewFindingV1 {
            id: "F-1".into(),
            requirement: "REQ-001".into(),
            severity: crate::harness::prompt_catalog::Severity::High,
            kind: crate::harness::prompt_catalog::FindingKind::Requirements,
            location: "inspect /home/example/private-file.rs for context".into(),
            evidence: vec!["plan:snapshot-2".into()],
            repro: Some("Open C:\\Users\\example\\private-file.rs".into()),
            proposed_correction: "x".repeat(5000),
        });
    let sanitized = sanitize_verdict(verdict);
    assert_eq!(
        sanitized.findings[0].location,
        "inspect [local path] for context"
    );
    assert_eq!(
        sanitized.findings[0].repro.as_deref(),
        Some("Open [local path]")
    );
    assert_eq!(
        sanitized.findings[0].proposed_correction.chars().count(),
        4096
    );
}
