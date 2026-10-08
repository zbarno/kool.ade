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
