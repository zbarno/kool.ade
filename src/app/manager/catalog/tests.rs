use super::*;

fn evidence(id: &str, content: &str) -> EvidenceItem {
    EvidenceItem {
        id: id.into(),
        workspace_uid: "workspace-1".into(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: format!("source for {id}"),
        content: content.into(),
    }
}

#[test]
fn shared_evidence_from_multiple_pending_reviews_renders_once() {
    let shared = evidence("product-spec:current-capabilities", "shared current spec");
    let manifest = deduplicate_evidence(vec![
        evidence("plan-review:feature-a", "finding A"),
        shared.clone(),
        evidence("plan-review:feature-b", "finding B"),
        shared,
    ])
    .expect("identical evidence IDs from separate reviews are deduplicated");
    assert_eq!(manifest.len(), 3);
    assert_eq!(
        manifest
            .iter()
            .filter(|item| item.id == "product-spec:current-capabilities")
            .count(),
        1
    );

    let rendered = crate::harness::prompt_catalog::render(&RoleInputV1 {
        role: PromptRole::Manager,
        workspace_uid: "workspace-1".into(),
        feature_uid: "workspace-1".into(),
        task_uid: None,
        original_goal: "Review pending plans".into(),
        approved_spec_ref: None,
        approved_spec_hash: None,
        decision_refs: Vec::new(),
        evidence_manifest: manifest,
        execution_policy_ref: "manager-policy/application-actions/v1".into(),
        output_schema_version: PromptRole::Manager.output_schema_version().into(),
        payload: RolePayloadV1::Manager {
            board_event_cursor: "cursor".into(),
            work_graph: Vec::new(),
            unresolved_findings: Vec::new(),
            latest_review_receipts: vec![
                "plan-review:feature-a".into(),
                "plan-review:feature-b".into(),
            ],
        },
    });
    assert!(
        rendered.is_ok(),
        "deduplicated multi-review manifest renders: {rendered:?}"
    );
}

#[test]
fn evidence_id_cannot_be_reused_for_conflicting_content() {
    assert!(
        deduplicate_evidence(vec![
            evidence("product-spec:current-capabilities", "older spec"),
            evidence("product-spec:current-capabilities", "newer spec"),
        ])
        .is_err()
    );
}

#[test]
fn shared_manager_dispositions_route_by_their_evidence_type() {
    use crate::harness::prompt_catalog::{FindingScopeV1, ManagerActionKind};

    let plan_id = "plan-review:snapshot".to_owned();
    let code_id = "code-review-finding:current".to_owned();
    let plan_targets = [(
        plan_id.clone(),
        reviews::ReviewTarget {
            feature_id: "CHG-004".into(),
            revision: "a".repeat(64),
            finding_ids: vec!["F-1".into()],
            finding_claims: Default::default(),
            evidence_ids: Vec::new(),
        },
    )]
    .into();
    let code_target = code_reviews::FindingTarget {
        task_uid: "00000000-0000-4000-8000-000000000001".into(),
        feature_uid: "00000000-0000-4000-8000-000000000002".into(),
        subject_hash: "b".repeat(64),
        finding_id: "finding-1".into(),
        finding_fingerprint: "c".repeat(64),
        scope: FindingScopeV1::NonBlocking,
    };
    let code_targets = [(code_id.clone(), code_target.clone())].into();
    let evidence: HashSet<String> = [plan_id.clone(), code_id.clone()].into();
    let actions = [
        crate::harness::prompt_catalog::ManagerActionV1 {
            kind: ManagerActionKind::AskUser,
            target: "CHG-004#F-1".into(),
            reason: "The plan reviewer needs this product decision before proceeding.".into(),
            evidence: vec![plan_id.clone()],
            expected_revision: "a".repeat(64),
            contradiction: None,
        },
        crate::harness::prompt_catalog::ManagerActionV1 {
            kind: ManagerActionKind::PromoteFollowUp,
            target: format!("{}#finding-1", code_target.task_uid),
            reason: "This independent improvement should be tracked outside the current task."
                .into(),
            evidence: vec![code_id],
            expected_revision: code_target.subject_hash,
            contradiction: None,
        },
    ];
    assert!(
        reviews::validate_dispositions(&actions, &plan_targets, &evidence, &HashMap::new()).is_ok()
    );
    assert!(code_reviews::validate_dispositions(&actions, &code_targets, &evidence).is_ok());
}
