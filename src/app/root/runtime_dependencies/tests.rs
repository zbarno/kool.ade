use super::*;

#[test]
fn prepared_dependency_label_removes_controls_from_package_and_version() {
    let request = DependencyRequest {
        id: "55555555-5555-4555-8555-555555555555".into(),
        task_id: "22222222-2222-4222-8222-222222222222".into(),
        need: crate::harness::DependencyNeed {
            ecosystem: crate::harness::PackageEcosystem::Npm,
            package: Some("safe\npackage".into()),
            version: Some("1.0\u{7f}0".into()),
            source: None,
            command: String::new(),
            reason: String::new(),
            kind: crate::harness::DependencyKind::NewProjectDependency,
            lockfile_identity: None,
            introduced_packages: Vec::new(),
        },
        category: crate::harness::DependencyFailureCategory::Unknown,
        decision: crate::harness::DependencyDecision::AuthorizeForTask,
        rationale: String::new(),
        risk: String::new(),
        status: DependencyRequestStatus::Prepared,
        preparation: None,
    };

    let label = dependency_label(&request);

    assert_eq!(label, "npm: safepackage@1.00");
    assert!(!label.chars().any(char::is_control));
}
