use super::super::*;
use super::helpers::*;
use crate::core::gitops;
/// Production boundary: clone then ordinary connect must create only private
/// local state for a plain code repository. A store unit test cannot prove the
/// welcome clone/connect route leaves the code checkout clean. Uses a temporary
/// local Git repository and no external services.
#[test]
fn perform_clone_local_source_connects_without_planning_remote() {
    let sb = EnvSandbox::enter("chain", false);
    let src = local_source_repo("chain");
    let dest =
        perform_clone(src.to_str().unwrap(), "sw-clone").expect("offline local-source clone lands");
    assert_eq!(
        dest,
        sb.home.join("kool-ade-workspaces/sw-clone"),
        "$HOME/kool-ade-workspaces joined with the repo segment verbatim"
    );
    assert!(
        gitops::is_work_tree(&dest),
        "files arrived as a code checkout"
    );
    no_scratch_left(&sb.home);

    let project = attempt_connect(dest.to_str().unwrap()).expect("plain code repo connects");
    assert_eq!(project.state.repo_root, dest);
    assert!(
        project
            .state
            .planning_store
            .root
            .starts_with(crate::persistence::state_root())
    );
    assert!(!project.state.planning_store.root.starts_with(&dest));
    assert!(!dest.join(".koolade-packet").exists());
    assert!(!dest.join("planning").exists());
    let status = std::process::Command::new("git")
        .args(["-C", dest.to_str().unwrap(), "status", "--porcelain"])
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(
        status.stdout.is_empty(),
        "connect left the code checkout clean"
    );
    let _ = std::fs::remove_dir_all(&src);
}
