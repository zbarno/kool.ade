use super::prepared_dependency;
use crate::harness::{
    DependencyDecision, DependencyKind, DependencyNeed, DependencyRequest, DependencyRequestStatus,
    PackageEcosystem,
};

fn request(id: &str, task_id: &str, status: DependencyRequestStatus) -> DependencyRequest {
    DependencyRequest {
        id: id.into(),
        task_id: task_id.into(),
        need: DependencyNeed {
            ecosystem: PackageEcosystem::Npm,
            package: None,
            version: None,
            source: None,
            command: "npm ci".into(),
            reason: "Restore locked packages".into(),
            kind: DependencyKind::ExistingRestore,
            lockfile_identity: Some("sha256:lock".into()),
            introduced_packages: Vec::new(),
        },
        category: Default::default(),
        decision: DependencyDecision::AuthorizeForTask,
        rationale: "Approved for this task".into(),
        risk: "Public lockfile source".into(),
        status,
        preparation: None,
    }
}

#[test]
fn cache_snapshot_selects_the_exact_prepared_request_among_concurrent_requests() {
    let requests = [
        request("first-request", "task", DependencyRequestStatus::Prepared),
        request("later-request", "task", DependencyRequestStatus::Prepared),
    ];

    let approved = prepared_dependency(&requests, "first-request", "task").unwrap();
    assert_eq!(approved.id, "first-request");
    assert!(prepared_dependency(&requests, "first-request", "other-task").is_err());
    assert!(prepared_dependency(&requests, "missing-request", "task").is_err());

    let pending = [request(
        "first-request",
        "task",
        DependencyRequestStatus::Pending,
    )];
    assert!(prepared_dependency(&pending, "first-request", "task").is_err());
}
