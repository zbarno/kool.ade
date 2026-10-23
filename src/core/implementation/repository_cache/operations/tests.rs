use super::*;
use std::sync::{Arc, atomic::AtomicBool, mpsc};
use std::time::{Duration, Instant};

#[test]
fn owned_clone_metadata_checks_reject_linked_worktrees_and_alternates() {
    let root = std::env::temp_dir().join(format!(
        "koolade-clone-metadata-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let repo = root.join("repo");
    let linked = root.join("linked");
    fs::create_dir_all(&repo).unwrap();
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: "gh".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    runner.git(&repo, &["init", "-q", "-b", "main"]).unwrap();
    runner
        .git(
            &repo,
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "--allow-empty",
                "-qm",
                "baseline",
            ],
        )
        .unwrap();
    verify_independent_repository(&repo, &runner).unwrap();
    runner
        .git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "linked",
                linked.to_str().unwrap(),
            ],
        )
        .unwrap();
    assert!(verify_independent_repository(&linked, &runner).is_err());
    let alternates = repo.join(".git/objects/info/alternates");
    fs::write(&alternates, root.to_string_lossy().as_bytes()).unwrap();
    assert!(
        verify_independent_repository(&repo, &runner)
            .unwrap_err()
            .to_string()
            .contains("external Git object store")
    );
    fs::remove_dir_all(root).unwrap();
}

