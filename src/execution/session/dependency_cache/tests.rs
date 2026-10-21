use super::valid_dependency_request_id;

#[test]
fn dependency_request_ids_require_the_canonical_prefix_and_uuid() {
    assert!(valid_dependency_request_id(
        "dependency-00000000-0000-4000-8000-000000000001"
    ));
    assert!(!valid_dependency_request_id(
        "00000000-0000-4000-8000-000000000001"
    ));
    assert!(!valid_dependency_request_id(
        "dependency-00000000-0000-4000-8000-00000000000A"
    ));
    assert!(!valid_dependency_request_id("dependency-not-a-uuid"));
}
