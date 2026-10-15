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

#[test]
fn pr_preparer_keeps_every_near_limit_diff_chunk_in_the_rendered_prompt() {
    let mut input = base(
        PromptRole::PrPreparer,
        RolePayloadV1::Pr {
            reviewed_task_refs: vec![],
            approved_subtasks: vec![],
            base_ref: "refs/heads/main".into(),
            head_ref: "refs/heads/feature".into(),
        },
    );
    input.evidence_manifest.clear();
    let chunks = (0..7)
        .map(|index| char::from(b'A' + index as u8).to_string().repeat(8 * 1024))
        .collect::<Vec<_>>();
    for (index, content) in chunks.iter().enumerate() {
        input.evidence_manifest.push(EvidenceItem {
            id: format!("integrated-diff-{index}"),
            workspace_uid: input.workspace_uid.clone(),
            origin: EvidenceOrigin::Repository,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: "complete immutable integrated Git diff".into(),
            content: content.clone(),
        });
    }

    let rendered = render(&input).unwrap();
    for chunk in chunks {
        assert!(
            rendered.user.contains(&chunk),
            "rendered diff omitted a chunk"
        );
    }
}

#[test]
fn evidence_that_exceeds_rendered_budget_is_rejected_without_truncation() {
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
    for index in 0..6 {
        input.evidence_manifest.push(EvidenceItem {
            id: format!("evidence-{index}"),
            workspace_uid: input.workspace_uid.clone(),
            origin: EvidenceOrigin::Repository,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: "source.md".into(),
            content: "x".repeat(12 * 1024),
        });
    }
    assert!(render(&input).unwrap_err().contains("64 KiB"));
}
