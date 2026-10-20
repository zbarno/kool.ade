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
