#![cfg(unix)]
use super::support::TestDirectory;

#[test]
fn missing_workspace_does_not_hide_healthy_entries_and_reconnect_preserves_identity() {
    // Real Git identity/ancestry and private association boundary. Two tiny
    // local planning repositories, no network or Docker. A mocked listing
    // cannot prove that reconnection preserves the planning repository.
    let temp = TestDirectory::new("reconnect");
    let manager = temp.manager("operator");
    let first = manager.create("First").unwrap();
    let second = manager.create("Second").unwrap();
    manager.record_successful_open(first.id).unwrap();
    let moved = temp.path.join("moved");
    std::fs::rename(&first.planning_repo, &moved).unwrap();
    let listing = manager.list_launcher().unwrap();
    assert_eq!(listing.available.len(), 1);
    assert_eq!(listing.available[0].id, second.id);
    assert_eq!(listing.unavailable, vec![first.id]);
    assert!(manager.reconnect(first.id, &second.planning_repo).is_err());
    let restored = manager.reconnect(first.id, &moved).unwrap();
    assert_eq!(restored.id, first.id);
    assert_eq!(
        restored.association.code_checkouts,
        first.association.code_checkouts
    );
    assert_eq!(manager.list_launcher().unwrap().available.len(), 2);
    assert!(manager.reconnect(first.id, &moved).is_err());
}

#[test]
fn reconnect_rejects_a_clone_and_does_not_publish_invalid_candidate() {
    // Real Git relocation boundary: a clone of the last-opened history cannot
    // stand in for an original checkout with subsequent work. Local Git only;
    // two tiny repositories, no network, Docker or credentials.
    use super::support::git_ok;
    let temp = TestDirectory::new("reconnect-history");
    let manager = temp.manager("operator");
    let workspace = manager.create("Original").unwrap();
    manager.record_successful_open(workspace.id).unwrap();
    let clone = temp.path.join("clone");
    git_ok(
        None,
        &[
            "clone",
            workspace.planning_repo.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    git_ok(
        Some(&workspace.planning_repo),
        &[
            "-c",
            "user.name=Test Operator",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Later planning work",
        ],
    );
    let moved = temp.path.join("moved");
    std::fs::rename(&workspace.planning_repo, &moved).unwrap();
    let before = std::fs::read(manager.association_path(workspace.id)).unwrap();
    assert!(manager.reconnect(workspace.id, &clone).is_err());
    // The real moved checkout also must pass complete repository validation
    // before its association is published.
    git_ok(
        Some(&moved),
        &["config", "core.hooksPath", "/synthetic/hooks"],
    );
    assert!(manager.reconnect(workspace.id, &moved).is_err());
    assert_eq!(
        std::fs::read(manager.association_path(workspace.id)).unwrap(),
        before
    );
    git_ok(Some(&moved), &["config", "--unset", "core.hooksPath"]);
    assert!(manager.reconnect(workspace.id, &moved).is_ok());
}

#[test]
fn multiple_private_profiles_preserve_the_same_relocation_marker() {
    // Local filesystem/Git association boundary; two private profiles and one
    // shared checkout. No network or Docker; a unit token comparison cannot
    // establish that recording an open leaves another profile reconnectable.
    let temp = TestDirectory::new("reconnect-profiles");
    let first = temp.manager("first");
    let second = temp.manager("second");
    let workspace = first.create("Shared locally").unwrap();
    second.join(&workspace.planning_repo).unwrap();
    first.record_successful_open(workspace.id).unwrap();
    second.record_successful_open(workspace.id).unwrap();
    let moved = temp.path.join("moved");
    std::fs::rename(&workspace.planning_repo, &moved).unwrap();
    first.reconnect(workspace.id, &moved).unwrap();
    second.reconnect(workspace.id, &moved).unwrap();
}

#[test]
fn copied_backup_with_original_marker_is_not_a_relocation() {
    // Real filesystem copy vs rename boundary on supported Unix filesystems.
    // A Git clone test alone misses backups containing private Git markers.
    // Local Git and filesystem only; one small repository and one copied tree.
    let temp = TestDirectory::new("reconnect-backup");
    let manager = temp.manager("operator");
    let workspace = manager.create("Original").unwrap();
    manager.record_successful_open(workspace.id).unwrap();
    let backup = temp.path.join("backup");
    fn copy_tree(source: &std::path::Path, destination: &std::path::Path) {
        std::fs::create_dir(destination).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &destination.join(entry.file_name()));
            } else {
                std::fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
            }
        }
    }
    copy_tree(&workspace.planning_repo, &backup);
    let moved = temp.path.join("original-moved");
    std::fs::rename(&workspace.planning_repo, &moved).unwrap();
    let before = std::fs::read(manager.association_path(workspace.id)).unwrap();
    assert!(manager.reconnect(workspace.id, &backup).is_err());
    assert_eq!(
        std::fs::read(manager.association_path(workspace.id)).unwrap(),
        before
    );
    manager.reconnect(workspace.id, &moved).unwrap();
}
