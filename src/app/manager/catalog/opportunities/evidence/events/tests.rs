use super::specification_finding_evidence;
use crate::core::lifecycle::{ProductSpecFindingV1, SpecDeltaClassificationV1};

#[test]
fn specification_findings_become_stable_workspace_private_evidence() {
    let finding = ProductSpecFindingV1 {
        feature_uid: "00000000-0000-4000-8000-000000000001".into(),
        revision: 4,
        requirement_id: "REQ-7".into(),
        event_id: "00000000-0000-4000-8000-000000000002".into(),
        decision_refs: Vec::new(),
        classification: SpecDeltaClassificationV1::Documentation,
        evidence_refs: vec!["spec-history:7".into()],
        test_refs: vec!["docs/checks.md".into()],
        affected_task_ids: vec!["00000000-0000-4000-8000-000000000003".into()],
        needs_user_reapproval: false,
    };

    let first = specification_finding_evidence(
        "00000000-0000-4000-8000-000000000004",
        std::slice::from_ref(&finding),
    );
    let second = specification_finding_evidence("00000000-0000-4000-8000-000000000004", &[finding]);

    assert_eq!(first, second, "evidence IDs stay stable across refreshes");
    assert_eq!(first.len(), 1);
    assert!(first[0].id.starts_with("spec-finding:"));
    assert_eq!(
        first[0].confidentiality,
        crate::harness::prompt_catalog::EvidenceConfidentiality::WorkspacePrivate
    );
    assert!(first[0].content.contains("classification=Documentation"));
    assert!(first[0].content.contains("test_refs=docs/checks.md"));
}
