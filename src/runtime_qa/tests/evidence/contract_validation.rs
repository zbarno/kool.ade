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
