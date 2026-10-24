use super::report_fixture;
use crate::runtime_qa::*;

#[test]
fn incomplete_coverage_and_failed_assertions_never_pass() {
    let mut fixture = report_fixture();
    fixture.evidence.assertions.clear();
    assert!(
        fixture
            .evidence
            .validate(&fixture.request, &fixture.scenarios)
            .is_err()
    );

    fixture = report_fixture();
    fixture.evidence.assertions[0].status = QaAssertionStatus::Failed;
    fixture.evidence.assertions[0].observed = "The expected view did not appear".into();
    fixture.result.evidence_manifest_sha256 = fixture.evidence.sha256().unwrap();
    assert!(
        validate_report(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result
        )
        .is_err()
    );

    fixture.result.outcome = QaOutcome::Fail;
    fixture.result.cases[0].status = QaCaseStatus::Failed;
    fixture.result.review_coverage[0].status = QaReviewCoverageStatusV1::Fail;
    assert!(
        validate_report(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result
        )
        .is_ok()
    );
}

#[test]
fn evidence_manifest_is_bound_to_the_exact_runtime_target() {
    let mut fixture = report_fixture();
    fixture.evidence.target_id = Some("different-target".into());
    fixture.result.evidence_manifest_sha256 = fixture.evidence.sha256().unwrap();
    assert!(
        validate_report(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result
        )
        .is_err()
    );

    fixture = report_fixture();
    fixture.request.target_id = None;
    fixture.evidence.target_id = None;
    fixture.result.evidence_manifest_sha256 = fixture.evidence.sha256().unwrap();
    assert!(
        validate_report(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result
        )
        .is_ok(),
        "historical reports remain readable without a target binding"
    );
    assert!(
        validate_current_report(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result
        )
        .is_err(),
        "new reports cannot pass without a target binding"
    );
}
