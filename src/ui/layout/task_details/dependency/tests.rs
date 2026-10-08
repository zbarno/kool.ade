#[test]
fn dependency_activity_rows_keep_preparation_and_retry_details_visible() {
    let request = crate::harness::DependencyRequest {
        id: "request-1".into(),
        task_id: "task-1".into(),
        need: crate::harness::DependencyNeed {
            ecosystem: crate::harness::PackageEcosystem::Npm,
            package: Some("zod".into()),
            version: Some("^4.0.0".into()),
            source: Some("https://registry.npmjs.org".into()),
            command: "npm install zod@^4.0.0".into(),
            reason: "Task needs schema validation".into(),
            kind: crate::harness::DependencyKind::NewProjectDependency,
            lockfile_identity: None,
            introduced_packages: Vec::new(),
        },
        category: crate::harness::DependencyFailureCategory::DependencyNewPackageRequested,
        decision: crate::harness::DependencyDecision::AuthorizeForTask,
        rationale: "Prepared for the task".into(),
        risk: "Manifest changes require review".into(),
        status: crate::harness::DependencyRequestStatus::Prepared,
        preparation: Some(crate::harness::DependencyPreparationTelemetry {
            status: Some(crate::harness::DependencyPreparationStatus::Prepared),
            package_count: 2,
            cache_hits: 1,
            packages_downloaded: 1,
            bytes_downloaded: 32,
            authorization_source: Some(crate::harness::DependencyAuthorizationSource::Manager),
            retry_result: Some(crate::harness::DependencyRetryResult::Succeeded),
        }),
    };

    let rows = super::history::history_rows(&request).join("\n");

    for expected in [
        "Operation: npm · Add project dependency",
        "Packages: zod@^4.0.0",
        "Source: https://registry.npmjs.org",
        "Preparation: Prepared",
        "2 total · 1 cache hits · 1 downloaded · 32 bytes",
        "Authorization: Man.ager",
        "Offline retry: Succeeded",
    ] {
        assert!(rows.contains(expected), "missing {expected:?} in {rows}");
    }
}
