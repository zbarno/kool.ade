use super::{request_id, toolchain_request};

#[test]
fn missing_runtime_toolchain_becomes_a_non_executable_manager_request() {
    let request = toolchain_request("task-uid", "planning-item", "web-target", "python");

    assert_eq!(request.task_id, "planning-item");
    assert_eq!(request.need.package.as_deref(), Some("python"));
    assert_eq!(
        request.need.kind,
        crate::harness::DependencyKind::SystemTool
    );
    assert_eq!(
        request.need.ecosystem,
        crate::harness::PackageEcosystem::System
    );
    assert_eq!(
        request.status,
        crate::harness::DependencyRequestStatus::Pending
    );
    assert_eq!(
        request.decision,
        crate::harness::DependencyDecision::RequiresUserAuthorization
    );
    assert!(request.need.command.contains("approved runtime toolchain"));
    assert!(
        request
            .need
            .source
            .as_deref()
            .unwrap()
            .starts_with("runtime-capability://")
    );
    assert!(request.preparation.is_none());
    assert_eq!(request.id, request_id("task-uid", "web-target", "python"));
}
