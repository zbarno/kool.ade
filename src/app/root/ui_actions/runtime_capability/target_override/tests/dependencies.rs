use super::super::validate_required_dependencies;
use std::collections::BTreeSet;

#[test]
fn selected_dependencies_must_be_unique_and_in_the_validated_inventory() {
    let first = "11111111-1111-4111-8111-111111111111".to_owned();
    let second = "22222222-2222-4222-8222-222222222222".to_owned();
    let available = BTreeSet::from([first.clone(), second.clone()]);

    assert_eq!(
        validate_required_dependencies(&[second.clone(), first.clone()], &available).unwrap(),
        [first.clone(), second]
    );
    assert!(
        validate_required_dependencies(
            &["33333333-3333-4333-8333-333333333333".into()],
            &available
        )
        .is_err()
    );
    assert!(validate_required_dependencies(&[first.clone(), first], &available).is_err());
}
