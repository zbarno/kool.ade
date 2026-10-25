use super::dependencies::system_packages_for_requirements;

#[test]
fn recipe_tool_ids_map_to_sorted_bounded_debian_package_requests() {
    let tools = [
        "python".into(),
        "node".into(),
        "python3".into(),
        "go".into(),
    ];
    assert_eq!(
        system_packages_for_requirements(&tools).unwrap(),
        ["golang-go", "nodejs", "python-is-python3", "python3"]
    );
    assert_eq!(
        system_packages_for_requirements(&["node".into(), "nodejs".into()]).unwrap(),
        ["nodejs"]
    );
    assert!(system_packages_for_requirements(&["dotnet".into()]).is_err());
    assert!(system_packages_for_requirements(&["../escape".into()]).is_err());
}
