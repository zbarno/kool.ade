use super::super::validate_required_dependencies;
use std::collections::BTreeSet;

#[test]
fn selected_dependencies_must_be_unique_and_in_the_validated_inventory() {
    let available = BTreeSet::from(["prepared-a".to_owned(), "prepared-b".to_owned()]);

    assert_eq!(
        validate_required_dependencies(&["prepared-b".into(), "prepared-a".into()], &available,)
            .unwrap(),
        ["prepared-a", "prepared-b"]
    );
    assert!(validate_required_dependencies(&["forged".into()], &available).is_err());
    assert!(
        validate_required_dependencies(&["prepared-a".into(), "prepared-a".into()], &available,)
            .is_err()
    );
}
