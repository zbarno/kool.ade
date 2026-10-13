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
