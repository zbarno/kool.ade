use super::dependencies::{
    RequiredDependency, parse_required_dependencies, system_packages_for_requirements,
};
use crate::harness::PackageEcosystem;

#[test]
fn target_dependencies_require_supported_namespaced_valid_identities() {
    let parsed = parse_required_dependencies(&[
        "system:libssl3".into(),
        "npm:zod".into(),
        "cargo:serde".into(),
    ])
    .unwrap();
    assert_eq!(parsed.len(), 3);
    assert_eq!(parsed[0].ecosystem, PackageEcosystem::System);
    assert_eq!(parsed[1].package, "zod");
    assert_eq!(parsed[2].package, "serde");
    assert!(parse_required_dependencies(&["zod".into()]).is_err());
    assert!(parse_required_dependencies(&["maven:org.example:library".into()]).is_err());
    assert!(parse_required_dependencies(&["npm:../escape".into()]).is_err());
}

#[test]
fn system_tool_and_target_prerequisites_share_a_sorted_deduplicated_request() {
    let dependencies = [RequiredDependency {
        id: "system:nodejs".into(),
        ecosystem: PackageEcosystem::System,
        package: "nodejs".into(),
    }];
    assert_eq!(
        system_packages_for_requirements(&["node".into()], &[&dependencies[0]]).unwrap(),
        ["nodejs"]
    );
    assert!(
        system_packages_for_requirements(
            &[],
            &[&RequiredDependency {
                id: "system:bad".into(),
                ecosystem: PackageEcosystem::System,
                package: "../escape".into(),
            }]
        )
        .is_err()
    );
}

#[test]
fn recipe_tool_ids_map_to_sorted_bounded_debian_package_requests() {
    let tools = [
        "python".into(),
        "node".into(),
        "python3".into(),
        "go".into(),
    ];
    assert_eq!(
        system_packages_for_requirements(&tools, &[]).unwrap(),
        ["golang-go", "nodejs", "python-is-python3", "python3"]
    );
    assert_eq!(
        system_packages_for_requirements(&["node".into(), "nodejs".into()], &[]).unwrap(),
        ["nodejs"]
    );
    assert!(system_packages_for_requirements(&["dotnet".into()], &[]).is_err());
    assert!(system_packages_for_requirements(&["../escape".into()], &[]).is_err());
}
