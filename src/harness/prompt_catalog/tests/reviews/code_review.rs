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
        r#"{{"subject_hash":"{}","pass":true,"outcome":"pass","examined_files":[],"tested_evidence_ids":["verified-test-report-0"],"unexamined_areas":[],"coverage":[{{"requirement":"R1","status":"fail","evidence":[]}}],"findings":[],"untested":[]}}"#,
        "c".repeat(64)
    );
    let context = super::schemas::OutputValidationContext {
        expected_subject_hash: Some(&"c".repeat(64)),
        evidence_ids: ["verified-test-report-0".to_owned()].into(),
        ..Default::default()
    };
    assert!(
        super::schemas::validate_output_with_context(
            PromptRole::CodeReviewer,
            &false_pass,
            &context,
        )
        .unwrap_err()
        .contains("cannot pass")
    );
}

#[test]
fn code_reviewer_finding_ids_are_unique_and_categories_are_typed() {
    let hash = "d".repeat(64);
    let duplicate = format!(
        r#"{{"subject_hash":"{hash}","pass":false,"outcome":"blocking_findings","examined_files":[],"tested_evidence_ids":[],"unexamined_areas":[],"coverage":[{{"requirement":"AC-1","status":"fail","evidence":["review:evidence"]}}],"findings":[{{"id":"F-1","requirement":"AC-1","severity":"informational","kind":"overengineering","scope":"blocking","location":"src/lib.rs:12","location_detail":{{"file":"src/lib.rs","line_start":12,"line_end":12}},"evidence":["review:evidence"],"repro":null,"rationale":"The diff adds unnecessary work on the required path.","affected_requirement_ids":["AC-1"],"suggested_action":"repair","proposed_correction":"Remove the unnecessary allocation."}},{{"id":"F-1","requirement":"AC-1","severity":"low","kind":"unnecessary_abstraction","scope":"non_blocking","location":"src/lib.rs:20","location_detail":{{"file":"src/lib.rs","line_start":20,"line_end":20}},"evidence":["review:evidence"],"repro":null,"rationale":"A single-use wrapper adds avoidable indirection.","affected_requirement_ids":["AC-1"],"suggested_action":"follow_up","proposed_correction":"Inline the single-use wrapper."}}],"untested":[]}}"#
    );
    let context = super::schemas::OutputValidationContext {
        expected_subject_hash: Some(&hash),
        evidence_ids: [
            "review:evidence".to_owned(),
            "verified-test-report-0".to_owned(),
        ]
        .into(),
        required_requirements: vec!["AC-1".into()],
        ..Default::default()
    };
    assert!(
        super::schemas::validate_output_with_context(
            PromptRole::CodeReviewer,
            &duplicate,
            &context,
        )
        .unwrap_err()
        .contains("unique")
    );
    let unique = duplicate.replace(
        "\"id\":\"F-1\",\"requirement\":\"AC-1\",\"severity\":\"low\"",
        "\"id\":\"F-2\",\"requirement\":\"AC-1\",\"severity\":\"low\"",
    );
    assert!(
        super::schemas::validate_output_with_context(PromptRole::CodeReviewer, &unique, &context,)
            .is_ok()
    );
    for legacy_category in ["requirements", "test_gap", "scope"] {
        let unsupported = unique.replace(
            "\"kind\":\"overengineering\"",
            &format!("\"kind\":\"{legacy_category}\""),
        );
        assert!(
            super::schemas::validate_output_with_context(
                PromptRole::CodeReviewer,
                &unsupported,
                &context,
            )
            .unwrap_err()
            .contains("category"),
            "unsupported legacy category {legacy_category} must be rejected"
        );
    }
}

#[test]
fn code_reviewer_requires_explicit_scope_and_high_findings_to_block() {
    let hash = "e".repeat(64);
    let context = super::schemas::OutputValidationContext {
        expected_subject_hash: Some(&hash),
        evidence_ids: [
            "review:evidence".to_owned(),
            "verified-test-report-0".to_owned(),
        ]
        .into(),
        required_requirements: vec!["AC-1".into()],
        ..Default::default()
    };
    let finding = |severity: &str, scope: &str| {
        let action = if scope == "blocking" {
            "repair"
        } else {
            "follow_up"
        };
        let (pass, outcome) = if scope == "blocking" {
            ("false", "blocking_findings")
        } else {
            ("true", "pass")
        };
        format!(
            r#"{{"subject_hash":"{hash}","pass":{pass},"outcome":"{outcome}","examined_files":[],"tested_evidence_ids":["verified-test-report-0"],"unexamined_areas":[],"coverage":[{{"requirement":"AC-1","status":"pass","evidence":["review:evidence"]}}],"findings":[{{"id":"F-1","requirement":"AC-1","severity":"{severity}","kind":"correctness","scope":"{scope}","location":"src/lib.rs:12","location_detail":{{"file":"src/lib.rs","line_start":12,"line_end":12}},"evidence":["review:evidence"],"repro":"The reported behavior is reproducible.","rationale":"The incorrect result violates AC-1 under the supplied reproduction.","affected_requirement_ids":["AC-1"],"suggested_action":"{action}","proposed_correction":"Correct the observed behavior."}}],"untested":[]}}"#,
            pass = pass,
            outcome = outcome,
        )
    };
    let missing_scope = finding("low", "non_blocking").replace(",\"scope\":\"non_blocking\"", "");
    assert!(
        super::schemas::validate_output_with_context(
            PromptRole::CodeReviewer,
            &missing_scope,
            &context,
        )
        .is_err()
    );
    let high_nonblocking = finding("high", "non_blocking");
    assert!(
        super::schemas::validate_output_with_context(
            PromptRole::CodeReviewer,
            &high_nonblocking,
            &context,
        )
        .is_err()
    );
    let nonblocking_low = finding("low", "non_blocking");
    assert!(
        super::schemas::validate_output_with_context(
            PromptRole::CodeReviewer,
            &nonblocking_low,
            &context,
        )
        .is_ok()
    );
}

#[test]
fn code_reviewer_must_account_for_tested_evidence_and_unexamined_areas() {
    let hash = "a".repeat(64);
    let context = super::schemas::OutputValidationContext {
        expected_subject_hash: Some(&hash),
        evidence_ids: ["verified-test-report-0".to_owned()].into(),
        ..Default::default()
    };
    let unknown_evidence = format!(
        r#"{{"subject_hash":"{hash}","pass":false,"outcome":"review_unable","examined_files":[],"tested_evidence_ids":["missing-evidence"],"unexamined_areas":[],"coverage":[],"findings":[],"untested":["AC-1"]}}"#
    );
    assert!(
        super::schemas::validate_output_with_context(
            PromptRole::CodeReviewer,
            &unknown_evidence,
            &context,
        )
        .unwrap_err()
        .contains("evidence absent from the application evidence manifest")
    );

    let undisclosed_pass = format!(
        r#"{{"subject_hash":"{hash}","pass":true,"outcome":"pass","examined_files":[],"tested_evidence_ids":["verified-test-report-0"],"unexamined_areas":["startup edge case"],"coverage":[],"findings":[],"untested":[]}}"#
    );
    assert!(
        super::schemas::validate_output_with_context(
            PromptRole::CodeReviewer,
            &undisclosed_pass,
            &context,
        )
        .unwrap_err()
        .contains("unexamined areas")
    );

    let untested_pass = format!(
        r#"{{"subject_hash":"{hash}","pass":true,"outcome":"pass","examined_files":[],"tested_evidence_ids":[],"unexamined_areas":[],"coverage":[],"findings":[],"untested":[]}}"#
    );
    assert!(
        super::schemas::validate_output_with_context(
            PromptRole::CodeReviewer,
            &untested_pass,
            &context,
        )
        .unwrap_err()
        .contains("without testing verified evidence")
    );
}

#[test]
fn legacy_code_reviewer_v1_accepts_findings_without_v2_scope_metadata() {
    let hash = "e".repeat(64);
    let legacy = format!(
        r#"{{"subject_hash":"{hash}","pass":false,"coverage":[{{"requirement":"AC-1","status":"pass","evidence":["review:evidence"]}}],"findings":[{{"id":"F-1","requirement":"AC-1","severity":"low","kind":"overengineering","location":"src/lib.rs:12","evidence":["review:evidence"],"repro":null,"proposed_correction":"Remove the single-use wrapper."}}],"untested":[]}}"#
    );
    let context = super::schemas::OutputValidationContext {
        expected_subject_hash: Some(&hash),
        evidence_ids: ["review:evidence".to_owned()].into(),
        required_requirements: vec!["AC-1".into()],
        ..Default::default()
    };
    assert!(super::schemas::validate_output_with_context(
        PromptRole::CodeReviewerV1,
        &legacy,
        &context,
    )
    .is_ok());
    assert!(
        super::schemas::validate_output_with_context(PromptRole::CodeReviewer, &legacy, &context,)
            .is_err()
    );
}
