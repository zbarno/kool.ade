use super::*;

#[test]
fn independent_reviews_reject_same_run_or_resumable_reviewer_session() {
    let mut input = base(
        PromptRole::CodeReviewer,
        RolePayloadV1::Review {
            subject_revision: "sha".into(),
            subject_hash: "b".repeat(64),
            author_run_id: "author".into(),
            reviewer_run_id: "reviewer".into(),
            author_session_id: Some("00000000-0000-4000-8000-000000000001".into()),
            reviewer_session_id: Some("00000000-0000-4000-8000-000000000002".into()),
            requirements_matrix: vec![],
            explicit_exclusions: vec![],
        },
    );
    assert!(render(&input).is_ok());
    if let RolePayloadV1::Review {
        reviewer_session_id,
        ..
    } = &mut input.payload
    {
        *reviewer_session_id = Some("00000000-0000-4000-8000-000000000001".into());
    }
    assert!(render(&input).is_err());
}

#[test]
fn review_prompt_rejects_missing_or_malformed_session_identity() {
    let mut input = base(
        PromptRole::CodeReviewer,
        RolePayloadV1::Review {
            subject_revision: "sha".into(),
            subject_hash: "b".repeat(64),
            author_run_id: "author".into(),
            reviewer_run_id: "reviewer".into(),
            author_session_id: None,
            reviewer_session_id: Some("00000000-0000-4000-8000-000000000002".into()),
            requirements_matrix: vec![],
            explicit_exclusions: vec![],
        },
    );
    assert!(render(&input).is_err());
    if let RolePayloadV1::Review {
        author_session_id,
        reviewer_session_id,
        ..
    } = &mut input.payload
    {
        *author_session_id = Some("00000000-0000-4000-8000-000000000001".into());
        *reviewer_session_id = Some("not-a-uuid".into());
    }
    assert!(render(&input).unwrap_err().contains("session identities"));
}

#[test]
fn strict_review_validation_rejects_wrong_subject_and_false_pass() {
    let missing_subject = r#"{"pass":false,"coverage":[],"findings":[],"untested":[]}"#;
    assert!(validate_output(PromptRole::CodeReviewer, missing_subject, None).is_err());

    let wrong_subject =
        r#"{"subject_hash":"wrong","pass":true,"coverage":[],"findings":[],"untested":[]}"#;
    assert!(
        validate_output(
            PromptRole::CodeReviewer,
            wrong_subject,
            Some(&"c".repeat(64))
        )
        .unwrap_err()
        .contains("subject hash")
    );
    let false_pass = format!(
        r#"{{"subject_hash":"{}","pass":true,"coverage":[{{"requirement":"R1","status":"fail","evidence":[]}}],"findings":[],"untested":[]}}"#,
        "c".repeat(64)
    );
    assert!(
        validate_output(PromptRole::CodeReviewer, &false_pass, Some(&"c".repeat(64)))
            .unwrap_err()
            .contains("cannot pass")
    );
}

#[test]
fn known_failed_requirement_cannot_be_hidden_by_zero_findings() {
    let hash = "c".repeat(64);
    let failed_without_finding = format!(
        r#"{{"subject_hash":"{hash}","pass":false,"coverage":[{{"requirement":"R1","status":"fail","evidence":[]}}],"findings":[],"untested":[]}}"#
    );
    let context = super::schemas::OutputValidationContext {
        expected_subject_hash: Some(&hash),
        evidence_ids: Default::default(),
        required_requirements: vec!["R1".into()],
        ..Default::default()
    };
    assert!(
        super::schemas::validate_output_with_context(
            PromptRole::CodeReviewer,
            &failed_without_finding,
            &context
        )
        .unwrap_err()
        .contains("zero-findings")
    );
}

#[test]
fn praise_only_review_cannot_omit_required_coverage() {
    let hash = "e".repeat(64);
    let praise_only = format!(
        r#"{{"subject_hash":"{hash}","pass":true,"coverage":[],"findings":[],"untested":[]}}"#
    );
    let context = super::schemas::OutputValidationContext {
        expected_subject_hash: Some(&hash),
        required_requirements: vec!["R1: persisted state survives restart".into()],
        ..Default::default()
    };
    assert!(
        super::schemas::validate_output_with_context(
            PromptRole::CodeReviewer,
            &praise_only,
            &context
        )
        .unwrap_err()
        .contains("omits a required acceptance check")
    );
}

#[test]
fn hallucinated_test_or_screenshot_evidence_cannot_validate() {
    let hash = "d".repeat(64);
    let output = format!(
        r#"{{"subject_hash":"{hash}","pass":false,"coverage":[{{"requirement":"R1","status":"fail","evidence":["screenshot:invented"]}}],"findings":[{{"id":"F1","requirement":"R1","severity":"medium","kind":"test_gap","location":"preview","evidence":["screenshot:invented"],"repro":null,"proposed_correction":"Capture an authorized screenshot."}}],"untested":[]}}"#
    );
    let context = super::schemas::OutputValidationContext {
        expected_subject_hash: Some(&hash),
        evidence_ids: ["test:known".to_owned()].into(),
        required_requirements: vec!["R1".into()],
        ..Default::default()
    };
    assert!(
        super::schemas::validate_output_with_context(
            PromptRole::RuntimeQaReviewerV2,
            &output,
            &context
        )
        .unwrap_err()
        .contains("evidence manifest")
    );
}
