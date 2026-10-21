#[test]
fn dependency_request_ids_require_the_canonical_prefix_and_uuid() {
    use crate::execution::contract::validate_dependency_request_id as validate;

    assert!(validate("dependency-00000000-0000-4000-8000-000000000001").is_ok());
    assert!(validate("00000000-0000-4000-8000-000000000001").is_ok());
    assert!(validate("dependency-00000000-0000-4000-8000-00000000000A").is_err());
    assert!(validate("dependency-not-a-uuid").is_err());
}
