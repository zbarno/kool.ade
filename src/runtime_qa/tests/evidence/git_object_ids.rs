use super::*;

fn bind_subject(fixture: &mut ReportFixture, subject: &str) {
    fixture.request.code_head_sha = subject.into();
    fixture.evidence.code_sha = subject.into();
    fixture.result.subject_sha = subject.into();
    fixture.result.evidence_manifest_sha256 = fixture.evidence.sha256().unwrap();
}

#[test]
fn report_validation_accepts_exact_git_object_ids_and_rejects_invalid_subjects() {
    for length in [40, 64] {
        let mut fixture = report_fixture();
        bind_subject(&mut fixture, &"a".repeat(length));
        validate_report(
            &fixture.request,
            &fixture.scenarios,
            &fixture.evidence,
            &fixture.result,
        )
        .unwrap();

        fixture.result.subject_sha = "b".repeat(length);
        assert!(
            validate_report(
                &fixture.request,
                &fixture.scenarios,
                &fixture.evidence,
                &fixture.result,
            )
            .is_err(),
            "a different valid Git object ID must not identify the reviewed build"
        );
    }

    for subject in [
        "a".repeat(39),
        "a".repeat(41),
        "a".repeat(63),
        "g".repeat(40),
    ] {
        let mut fixture = report_fixture();
        bind_subject(&mut fixture, &subject);
        assert!(
            validate_report(
                &fixture.request,
                &fixture.scenarios,
                &fixture.evidence,
                &fixture.result,
            )
            .is_err(),
            "invalid Git object ID was accepted: {subject}"
        );
    }
}

/// Production boundary: durable QA publication and reload must preserve the
/// exact task Git object ID for both supported repository object formats.
/// Validator-only checks cannot prove the store invokes the same contract on
/// both writes and reads. Uses synthetic local evidence, no Docker/SSH/network;
/// setup is one temporary directory and tiny PNG per format.
#[test]
fn durable_qa_reports_roundtrip_sha1_and_sha256_git_subjects() {
    for length in [40, 64] {
        let mut fixture = report_fixture();
        bind_subject(&mut fixture, &"a".repeat(length));
        let root = tempfile::tempdir().unwrap();
        let store = RuntimeQaStore::new(root.path().join("qa"));
        store
            .save_run_with_screenshots(
                &fixture.request,
                &fixture.scenarios,
                &fixture.evidence,
                &fixture.result,
                std::slice::from_ref(&fixture.screenshot),
            )
            .unwrap();
        let (request, _, evidence, result) = store.load_run(&fixture.request.qa_run_id).unwrap();
        assert_eq!(request, fixture.request);
        assert_eq!(evidence, fixture.evidence);
        assert_eq!(result, fixture.result);
    }
}
