use super::*;

#[test]
fn oversized_role_payload_is_rejected_before_rendering() {
    let input = base(
        PromptRole::Manager,
        RolePayloadV1::Manager {
            board_event_cursor: "cursor".into(),
            work_graph: vec!["x".repeat(4097)],
            unresolved_findings: vec![],
            latest_review_receipts: vec![],
        },
    );
    assert!(render(&input).unwrap_err().contains("role payload"));
}

#[test]
fn evidence_metadata_counts_toward_the_total_quoted_evidence_limit() {
    let mut input = base(
        PromptRole::Manager,
        RolePayloadV1::Manager {
            board_event_cursor: "cursor".into(),
            work_graph: vec![],
            unresolved_findings: vec![],
            latest_review_receipts: vec![],
        },
    );
    input.evidence_manifest.clear();
    for index in 0..128 {
        input.evidence_manifest.push(EvidenceItem {
            id: format!("evidence-{index}"),
            workspace_uid: input.workspace_uid.clone(),
            origin: EvidenceOrigin::Repository,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("{}.md", "s".repeat(1018)),
            content: "short evidence".into(),
        });
    }
    assert!(render(&input).unwrap_err().contains("64 KiB"));
}
