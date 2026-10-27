use super::report_fixture;
use crate::runtime_qa::*;

use sha2::Digest as _;

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

#[test]
// Pins the forged-evidence guard: duplicate screenshot identifiers would
// clobber one persisted artifact under a single storage ref, so the manifest
// itself must refuse them. Validator-only test, temporary structures only.
fn duplicate_screenshot_identifiers_are_rejected_by_the_manifest() {
    let mut fixture = report_fixture();
    let duplicate = fixture.evidence.screenshots[0].clone();
    fixture.evidence.screenshots.push(duplicate);
    assert!(
        fixture
            .evidence
            .validate(&fixture.request, &fixture.scenarios)
            .is_err(),
        "duplicate screenshot ids must invalidate the evidence manifest"
    );
}

#[test]
// Pins the build-binding guard: an evidence manifest claiming a different
// build artifact than the request approved must not validate, or a repaired
// code subject could ride an old build's evidence past the report hash.
fn build_artifact_sha_mismatch_invalidates_the_report() {
    let mut fixture = report_fixture();
    fixture.evidence.build_hash =
        format!("{:x}", sha2::Sha256::digest(b"a different build artifact"));
    fixture.result.evidence_manifest_sha256 = fixture.evidence.sha256().unwrap();
    assert!(
        validate_report(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result
        )
        .is_err(),
        "a mismatched build hash must invalidate the report"
    );
}

#[test]
// Pins the required-AC coverage guarantee at the result contract: an
// Untested review-coverage entry describes a required acceptance criterion
// nobody exercised, so a Pass outcome is forbidden even when every executed
// assertion passed. Validator-only test, no external dependencies.
fn untested_required_acceptance_criteria_block_a_pass_outcome() {
    let fixture = report_fixture();
    let mut untested_result = fixture.result.clone();
    untested_result.review_coverage[0].status = QaReviewCoverageStatusV1::Untested;
    assert!(
        validate_report(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &untested_result
        )
        .is_err(),
        "a required criterion left Untested must block Pass"
    );
}
