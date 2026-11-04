use super::*;

#[test]
fn removed_manager_and_planner_manifests_are_unknown() {
    for role_version in [
        "manager/v2",
        "manager/v3",
        "manager/v4",
        "manager/v5",
        "manager/v6",
        "manager/v7",
        "planner/v2",
    ] {
        let manifest = PromptManifest {
            role_version: role_version.into(),
            system_template_sha256: "0".repeat(64),
            policy_template_sha256: "0".repeat(64),
            user_template_sha256: "0".repeat(64),
            output_schema_version: "obsolete".into(),
            privilege_profile: "obsolete".into(),
        };
        assert!(
            validate_prompt_manifest(&manifest)
                .unwrap_err()
                .contains("unknown prompt version"),
            "removed role {role_version} must not remain runtime-addressable"
        );
    }
}

#[test]
fn code_reviewer_v1_manifest_remains_readable_beside_the_v2_role() {
    let input = |role| {
        base(
            role,
            RolePayloadV1::Review {
                subject_revision: "base...head".into(),
                subject_hash: "c".repeat(64),
                author_run_id: "author-run".into(),
                reviewer_run_id: "reviewer-run".into(),
                author_session_id: Some("00000000-0000-4000-8000-000000000001".into()),
                reviewer_session_id: Some("00000000-0000-4000-8000-000000000002".into()),
                requirements_matrix: vec!["AC-1".into()],
                explicit_exclusions: vec![],
            },
        )
    };

    let v1 = render(&input(PromptRole::CodeReviewerV1)).unwrap().manifest;
    assert_eq!(v1.role_version, "code_review/v1");
    assert_eq!(
        validate_prompt_manifest(&v1).unwrap(),
        PromptRole::CodeReviewerV1
    );

    let v2 = render(&input(PromptRole::CodeReviewer)).unwrap().manifest;
    assert_eq!(v2.role_version, "code_review/v2");
    assert_eq!(
        validate_prompt_manifest(&v2).unwrap(),
        PromptRole::CodeReviewer
    );
    assert_ne!(v1.system_template_sha256, v2.system_template_sha256);
}

#[test]
fn runtime_qa_v1_identity_remains_readable_beside_the_v2_evidence_role() {
    let input = |role| {
        base(
            role,
            RolePayloadV1::Review {
                subject_revision: "qa-run".into(),
                subject_hash: "c".repeat(64),
                author_run_id: "author".into(),
                reviewer_run_id: "reviewer".into(),
                author_session_id: Some("00000000-0000-4000-8000-000000000001".into()),
                reviewer_session_id: Some("00000000-0000-4000-8000-000000000002".into()),
                requirements_matrix: vec!["REQ-1".into()],
                explicit_exclusions: vec![],
            },
        )
    };

    let v1 = render(&input(PromptRole::RuntimeQaReviewer))
        .unwrap()
        .manifest;
    assert_eq!(v1.role_version, "runtime_qa/v1");
    assert_eq!(
        v1.system_template_sha256,
        "b7862c1e4510b0c4b25bbcabee0571ecc5bc10db8c5822bf119d76e060aa12f9"
    );
    assert_eq!(
        validate_prompt_manifest(&v1).unwrap(),
        PromptRole::RuntimeQaReviewer
    );

    let v2 = render(&input(PromptRole::RuntimeQaReviewerV2))
        .unwrap()
        .manifest;
    assert_eq!(v2.role_version, "runtime_qa/v2");
    assert_ne!(v2.system_template_sha256, v1.system_template_sha256);
    assert_ne!(v2.user_template_sha256, v1.user_template_sha256);
    assert_eq!(
        validate_prompt_manifest(&v2).unwrap(),
        PromptRole::RuntimeQaReviewerV2
    );

    let v3 = render(&input(PromptRole::RuntimeQaReviewerV3))
        .unwrap()
        .manifest;
    assert_eq!(v3.role_version, "runtime_qa/v3");
    assert_ne!(v3.system_template_sha256, v2.system_template_sha256);
    assert_ne!(v3.user_template_sha256, v2.user_template_sha256);
    assert_eq!(
        validate_prompt_manifest(&v3).unwrap(),
        PromptRole::RuntimeQaReviewerV3
    );
}

#[test]
fn runtime_qa_scenario_synthesizer_has_a_distinct_versioned_proposal_role() {
    let prompt = base(
        PromptRole::RuntimeQaScenarioSynthesizer,
        RolePayloadV1::QaScenarioSynthesis {
            immutable_request_json: "{}".into(),
        },
    );
    let manifest = render(&prompt).unwrap().manifest;
    assert_eq!(manifest.role_version, "runtime_qa_scenarios/v1");
    assert_eq!(manifest.output_schema_version, "QaScenarioDraftV1");
    assert_eq!(
        validate_prompt_manifest(&manifest).unwrap(),
        PromptRole::RuntimeQaScenarioSynthesizer
    );
}
