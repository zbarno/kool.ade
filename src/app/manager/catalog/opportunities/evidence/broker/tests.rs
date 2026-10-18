use super::*;
use crate::harness::{
    DependencyDecision, DependencyFailureCategory, DependencyKind, DependencyNeed,
    DependencyPreparationStatus, DependencyPreparationTelemetry, PackageEcosystem,
};

#[test]
fn broker_evidence_keeps_typed_denial_without_copying_private_request_details() {
    let request = DependencyRequest {
        id: "dependency-00000000-0000-4000-8000-000000000001".into(),
        task_id: "ticket-local-path".into(),
        need: DependencyNeed {
            ecosystem: PackageEcosystem::Other,
            package: Some("private-internal-package".into()),
            version: Some("1.2.3".into()),
            source: Some("https://registry.example.invalid/private".into()),
            command: "install private-internal-package".into(),
            reason: "customer secret project requirement".into(),
            kind: DependencyKind::SystemTool,
            lockfile_identity: None,
            introduced_packages: Vec::new(),
        },
        category: DependencyFailureCategory::DependencyManagerUnsupported,
        decision: DependencyDecision::Reject,
        rationale: "private request rationale".into(),
        risk: "private request risk".into(),
        status: DependencyRequestStatus::Denied,
        preparation: Some(DependencyPreparationTelemetry {
            status: Some(DependencyPreparationStatus::Unsupported),
            ..Default::default()
        }),
    };

    let evidence = evidence_item(
        "00000000-0000-4000-8000-000000000001",
        "00000000-0000-4000-8000-000000000002",
        &request,
    );
    assert!(evidence.id.starts_with("broker-prerequisite:"));
    assert_eq!(
        evidence.confidentiality,
        EvidenceConfidentiality::WorkspacePrivate
    );
    assert!(evidence.content.contains("status=Denied"));
    assert!(evidence.content.contains("DependencyManagerUnsupported"));
    for private in [
        "private-internal-package",
        "registry.example.invalid",
        "customer secret",
        "private request rationale",
    ] {
        assert!(!evidence.content.contains(private));
    }
}
