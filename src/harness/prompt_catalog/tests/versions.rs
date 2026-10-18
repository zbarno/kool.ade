use super::*;

#[test]
fn prior_manager_and_planner_versions_remain_valid_for_persisted_runs() {
    let manager = base(
        PromptRole::ManagerV2,
        RolePayloadV1::Manager {
            board_event_cursor: "cursor-1".into(),
            work_graph: vec![],
            unresolved_findings: vec![],
            latest_review_receipts: vec![],
        },
    );
    let manager_manifest = render(&manager).unwrap().manifest;
    assert_eq!(manager_manifest.role_version, "manager/v2");
    assert_eq!(manager_manifest.output_schema_version, "ManagerOutputV1");
    assert_eq!(
        manager_manifest.system_template_sha256,
        "bbbfb130bf3759aa8715d7f2f0bb6fc6598019b59f11da790d9a36dd5affee76"
    );
    assert_eq!(
        manager_manifest.user_template_sha256,
        "0a46e44ec7ee0b47405b363573048f4603a10f5dec24a7e91d09411866828bea"
    );
    assert_eq!(
        validate_prompt_manifest(&manager_manifest).unwrap(),
        PromptRole::ManagerV2
    );

    let planner = base(
        PromptRole::PlannerV2,
        RolePayloadV1::Planner {
            open_questions: vec![],
            previous_answers: vec![],
            research_evidence: vec![],
            requested_scope: "review current scope".into(),
        },
    );
    let planner_manifest = render(&planner).unwrap().manifest;
    assert_eq!(planner_manifest.role_version, "planner/v2");
    assert_eq!(
        planner_manifest.system_template_sha256,
        "68a8a468d96bfb12c657d191786f7777f523422d83821368479186567e1e79d8"
    );
    assert_eq!(
        planner_manifest.user_template_sha256,
        "efbb57e4fcece988252bc6f0c6369fa86da9fa51d9765b45df4667a087564ce8"
    );
    assert_eq!(
        validate_prompt_manifest(&planner_manifest).unwrap(),
        PromptRole::PlannerV2
    );
}

#[test]
fn manager_v3_persisted_prompt_manifest_remains_readable() {
    let prompt = base(
        PromptRole::ManagerV3,
        RolePayloadV1::Manager {
            board_event_cursor: "cursor-legacy".into(),
            work_graph: vec![],
            unresolved_findings: vec![],
            latest_review_receipts: vec![],
        },
    );
    let manifest = render(&prompt).unwrap().manifest;
    assert_eq!(manifest.role_version, "manager/v3");
    assert_eq!(manifest.output_schema_version, "ManagerOutputV2");
    assert_eq!(
        validate_prompt_manifest(&manifest).unwrap(),
        PromptRole::ManagerV3
    );
}

#[test]
fn manager_v4_and_v5_prompt_manifests_remain_readable_after_the_v6_evidence_rules() {
    let prompt = base(
        PromptRole::ManagerV4,
        RolePayloadV1::Manager {
            board_event_cursor: "cursor-v4".into(),
            work_graph: vec![],
            unresolved_findings: vec![],
            latest_review_receipts: vec![],
        },
    );
    let manifest = render(&prompt).unwrap().manifest;
    assert_eq!(manifest.role_version, "manager/v4");
    assert_eq!(
        validate_prompt_manifest(&manifest).unwrap(),
        PromptRole::ManagerV4
    );

    let prior = render(&base(
        PromptRole::ManagerV5,
        RolePayloadV1::Manager {
            board_event_cursor: "cursor-v5".into(),
            work_graph: vec![],
            unresolved_findings: vec![],
            latest_review_receipts: vec![],
        },
    ))
    .unwrap()
    .manifest;
    assert_eq!(prior.role_version, "manager/v5");
    assert_eq!(
        validate_prompt_manifest(&prior).unwrap(),
        PromptRole::ManagerV5
    );

    let current = render(&base(
        PromptRole::Manager,
        RolePayloadV1::Manager {
            board_event_cursor: "cursor-v6".into(),
            work_graph: vec![],
            unresolved_findings: vec![],
            latest_review_receipts: vec![],
        },
    ))
    .unwrap()
    .manifest;
    assert_eq!(current.role_version, "manager/v6");
    assert_eq!(
        validate_prompt_manifest(&current).unwrap(),
        PromptRole::Manager
    );
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
}
