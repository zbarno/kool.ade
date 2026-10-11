use crate::{
    artifacts::planning_store::{PlanningStore, StoreMode},
    core::planning_work::{Work, load, save},
};
use std::{path::Path, process::Command};

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn checkout(root: &Path, name: &str, branch: &str) -> std::path::PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(&path).unwrap();
    git(&path, &["init", "-q"]);
    git(&path, &["config", "user.name", "Koolade Fixture"]);
    git(&path, &["config", "user.email", "fixture@example.test"]);
    std::fs::write(path.join("README.md"), "fixture\n").unwrap();
    git(&path, &["add", "README.md"]);
    git(&path, &["commit", "-qm", "fixture checkout"]);
    git(&path, &["checkout", "-qb", branch]);
    path
}

#[test]
fn synced_board_is_independent_of_two_code_branch_checkouts() {
    let root = std::env::temp_dir().join(format!(
        "koolade-branch-independent-board-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "--bare", "planning.git"]);
    git(&root, &["clone", "planning.git", "seed"]);
    let seed = root.join("seed");
    git(&seed, &["config", "user.name", "Koolade Fixture"]);
    git(&seed, &["config", "user.email", "fixture@example.test"]);
    std::fs::write(seed.join("README.md"), "planning repository\n").unwrap();
    git(&seed, &["add", "README.md"]);
    git(&seed, &["commit", "-qm", "initialize planning repository"]);
    git(&seed, &["branch", "-M", "main"]);
    git(&seed, &["push", "-q", "origin", "main"]);
    let remote = root.join("planning.git");
    let remote = remote.to_str().unwrap();
    git(
        &root,
        &["clone", "--branch", "main", remote, "operator-a-planning"],
    );
    git(
        &root,
        &["clone", "--branch", "main", remote, "operator-b-planning"],
    );
    let planning_a = root.join("operator-a-planning");
    let planning_b = root.join("operator-b-planning");
    for planning in [&planning_a, &planning_b] {
        git(planning, &["config", "user.name", "Koolade Fixture"]);
        git(planning, &["config", "user.email", "fixture@example.test"]);
    }
    let project_id = uuid::Uuid::new_v4();
    let store_a = PlanningStore::new(project_id, &planning_a, StoreMode::ManagedShared);
    let store_b = PlanningStore::new(project_id, &planning_b, StoreMode::ManagedShared);

    let mut task = Work::new(
        "task:branch-independent".into(),
        "Implement shared task".into(),
        "The board belongs to the workspace.".into(),
        "Ready for implementation".into(),
    );
    task.source_branch = Some("release/1.2".into());
    task.destination_branch = Some("develop".into());
    save(&store_a, &[task.clone()]).unwrap();
    git(&planning_a, &["add", "-A"]);
    git(&planning_a, &["commit", "-qm", "save shared board task"]);
    git(&planning_a, &["push", "-q", "origin", "main"]);
    git(&planning_b, &["pull", "--ff-only", "origin", "main"]);

    let code_a = checkout(&root, "operator-a-code", "release/1.2");
    let code_b = checkout(&root, "operator-b-code", "feature/auth");
    assert_eq!(git(&code_a, &["branch", "--show-current"]), "release/1.2");
    assert_eq!(git(&code_b, &["branch", "--show-current"]), "feature/auth");
    let board_a = load(&store_a).unwrap();
    let board_b = load(&store_b).unwrap();
    assert_eq!(board_a, board_b);
    assert_eq!(board_b[0].uid, task.uid);
    assert_eq!(board_b[0].source_branch.as_deref(), Some("release/1.2"));
    assert_eq!(board_b[0].destination_branch.as_deref(), Some("develop"));

    std::fs::remove_dir_all(root).unwrap();
}
