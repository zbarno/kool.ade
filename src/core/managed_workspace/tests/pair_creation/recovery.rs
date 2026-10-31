use super::*;

#[test]
// Production boundary: staged code creation publishes into an operator-selected parent.
// Invariant: publication never replaces a destination created after the initial absence check.
// This cannot be proven with path-validation unit checks alone because the no-replace rename
// is the filesystem boundary. External services: none. Setup cost: two temporary directories.
fn concurrent_destination_is_not_replaced_by_project_publication() {
    let temp = TestDirectory::new("pair-publication-race");
    let staging = temp.path.join("staging");
    let destination = temp.path.join("destination");
    std::fs::create_dir(&staging).unwrap();
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("notes.txt"), "operator data\n").unwrap();

    let error =
        crate::core::managed_workspace::application::publish::directory(&staging, &destination)
            .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("could not publish project folder")
    );
    assert_eq!(
        std::fs::read_to_string(destination.join("notes.txt")).unwrap(),
        "operator data\n"
    );
    assert!(
        staging.is_dir(),
        "the failed publication keeps its staging tree"
    );
}

#[test]
fn planning_repository_uses_managed_location_and_resumes_by_checkout_identity() {
    let temp = TestDirectory::new("pair-named-plan");
    std::fs::create_dir_all(temp.path.join("workspaces")).unwrap();
    let spec = spec_in(&temp);
    let manager = temp.manager("operator");

    let workspace = manager.create_new_project(&spec).unwrap();
    let planning = workspace.planning_repo.clone();
    assert_eq!(
        planning,
        manager.project_dir(workspace.id).join("planning-repo")
    );
    assert!(!temp.path.join("workspaces/widget.packet").exists());
    assert!(planning.join("manifest.json").is_file());
    let before = rev_count(&planning);
    manager.create_new_project(&spec).unwrap();
    assert_eq!(rev_count(&planning), before);
}

#[test]
fn hostile_repository_names_are_rejected_before_touching_disk() {
    let temp = TestDirectory::new("pair-names");
    std::fs::create_dir_all(temp.path.join("workspaces")).unwrap();
    let manager = temp.manager("operator");

    for name in ["../escape", "a b", ".dotfile", "", "way/too/many/slashes"] {
        let mut spec = spec_in(&temp);
        spec.code_repo_name = name.to_owned();
        manager.create_new_project(&spec).unwrap_err();
    }
    let entries = std::fs::read_dir(temp.path.join("workspaces"))
        .unwrap()
        .count();
    assert_eq!(
        entries, 0,
        "validation must happen before any disk mutation"
    );
}
