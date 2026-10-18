use super::super::parse_dependency_ids;

#[test]
fn dependency_request_ids_are_trimmed_bounded_and_unique() {
    assert_eq!(
        parse_dependency_ids("\n dependency-one \ndependency-two\n").unwrap(),
        ["dependency-one", "dependency-two"]
    );
    assert!(parse_dependency_ids("dependency-one\ndependency-one").is_err());
    assert!(parse_dependency_ids("../outside").is_err());
    assert!(parse_dependency_ids(&"x".repeat(129)).is_err());
}
