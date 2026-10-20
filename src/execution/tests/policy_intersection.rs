use super::super::{contract::ExecutionModeV1, policy::ExecutionPolicyV1};
use super::{budget, request};
use std::collections::BTreeSet;

#[test]
fn request_deadline_cannot_extend_beyond_the_bounded_execution_lease() {
    let now = 1_000;
    let mut request = request();
    request.deadline_unix_ms = now + super::super::contract::MAX_EXECUTION_DURATION_MS;
    request.validate_at(now).unwrap();

    request.deadline_unix_ms += 1;
    assert!(request.validate_at(now).is_err());
}

#[test]
fn verification_policy_allows_the_python_and_go_stack_recipes() {
    let policy = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Verification,
        budget(1024 * 1024 * 1024, 2000),
        BTreeSet::new(),
    )
    .unwrap();
    assert!(policy.command_capabilities().contains("python"));
    assert!(policy.command_capabilities().contains("go"));
}

#[test]
fn policy_intersection_never_widens_capabilities_or_resource_limits() {
    let broad = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Implementation,
        budget(4 * 1024 * 1024 * 1024, 4000),
        BTreeSet::new(),
    )
    .unwrap();
    let mut narrow_json = serde_json::to_value(&broad).unwrap();
    narrow_json["command_capabilities"] = serde_json::json!(["git"]);
    narrow_json["tool_capabilities"] = serde_json::json!(["read_repository"]);
    narrow_json["filesystem"] = serde_json::json!(["workspace_read"]);
    let narrow: ExecutionPolicyV1 = serde_json::from_value(narrow_json).unwrap();
    let effective = broad.restrict_to(&narrow).unwrap();
    assert_eq!(
        effective.command_capabilities(),
        &BTreeSet::from(["git".into()])
    );
    assert_eq!(
        effective.tool_capabilities(),
        &BTreeSet::from(["read_repository".into()])
    );
    assert!(
        effective
            .filesystem_grants()
            .is_subset(broad.filesystem_grants())
    );
    assert!(
        effective
            .filesystem_grants()
            .is_subset(narrow.filesystem_grants())
    );
    assert!(effective.resource_limits().memory_bytes <= broad.resource_limits().memory_bytes);
    assert!(effective.resource_limits().memory_bytes <= narrow.resource_limits().memory_bytes);

    let mut widened_json = serde_json::to_value(&narrow).unwrap();
    widened_json["command_capabilities"] = serde_json::json!(["docker"]);
    let widened: ExecutionPolicyV1 = serde_json::from_value(widened_json).unwrap();
    assert!(widened.validate().is_err());

    let request = request();
    assert!(request.validate_under_policy(1, &broad).is_ok());
    let low_ceiling = ExecutionPolicyV1::for_mode(
        ExecutionModeV1::Implementation,
        budget(512 * 1024 * 1024, 1000),
        BTreeSet::new(),
    )
    .unwrap();
    assert!(request.validate_under_policy(1, &low_ceiling).is_err());
}
