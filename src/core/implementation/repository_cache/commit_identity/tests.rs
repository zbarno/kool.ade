use super::*;
use crate::core::implementation::Runner;
use std::sync::{Arc, atomic::AtomicBool, mpsc};
use std::time::{Duration, Instant};

#[test]
fn null_identity_is_recaptured_and_inherited_by_integration_state() {
    let root = std::env::temp_dir().join(format!(
        "koolade-git-identity-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let repository = root.join("repository");
    let task_dir = root.join("task");
    let integration_dir = task_dir.join("integration");
    fs::create_dir_all(&repository).unwrap();
    fs::create_dir_all(&task_dir).unwrap();
    let initialized = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repository)
        .status()
        .unwrap();
    assert!(initialized.success());
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: "gh".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,

        authorization_verifier: None,
    };
    runner
        .git(&repository, &["config", "--local", "user.name", "Fixture"])
        .unwrap();
    runner
        .git(
            &repository,
            &["config", "--local", "user.email", "fixture@example.test"],
        )
        .unwrap();
    fs::write(task_dir.join(TASK_IDENTITY_FILE), "null").unwrap();

    let identity = load_or_capture(&task_dir, &repository, &runner).unwrap();
    fs::create_dir_all(&integration_dir).unwrap();
    save_for_task(&integration_dir, &identity).unwrap();

    assert_eq!(read_for_task(&task_dir).unwrap(), identity);
    assert_eq!(read_for_task(&integration_dir).unwrap(), identity);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn configure_clone_preserves_or_overrides_identity_as_requested() {
    let root = std::env::temp_dir().join(format!(
        "koolade-configure-identity-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&root).unwrap();
    let (progress, _updates) = mpsc::channel();
    let runner = Runner {
        gh: "gh".into(),
        runtime_config_source: None,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: Arc::new(AtomicBool::new(false)),
        progress,
        authorization_verifier: None,
    };
    runner.git(&root, &["init", "-q"]).unwrap();
    runner
        .git(&root, &["config", "--local", "user.name", "Existing"])
        .unwrap();
    runner
        .git(
            &root,
            &["config", "--local", "user.email", "existing@example.test"],
        )
        .unwrap();
    let approved = GitCommitIdentity {
        name: "Approved Agent".into(),
        email: "approved@example.test".into(),
    };
    configure_clone(&root, &approved, true, &runner).unwrap();
    assert_eq!(
        runner
            .git(&root, &["config", "--local", "user.name"])
            .unwrap(),
        "Existing"
    );
    assert_eq!(
        runner
            .git(&root, &["config", "--local", "user.email"])
            .unwrap(),
        "existing@example.test"
    );
    configure_clone(&root, &approved, false, &runner).unwrap();
    assert_eq!(
        GitCommitIdentity::from_repository(&root, &runner).unwrap(),
        approved
    );
    assert_eq!(
        runner
            .git(&root, &["config", "--local", "user.useConfigOnly"])
            .unwrap(),
        "true"
    );
    fs::remove_dir_all(root).unwrap();
}
