use super::super::support::{StateHome, type_into, visible_text};
use crate::app::root::{KooladeApp, Screen, WelcomeEntry};
use std::sync::{Arc, Mutex};

/// Boundary: reviewed welcome choice -> isolated code-clone worker. The
/// injected worker proves the async handoff carries only the canonical code
/// URL and repository name; it needs no network, Docker, or SSH.
#[test]
fn clone_wizard_dispatches_code_only_worker_without_pair_provisioning() {
    let _identity =
        crate::core::gitops::test_support::shield_with_synthetic_identity("clone-wizard-worker");
    let base = std::env::temp_dir().join(format!(
        "koolade-clone-wizard-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    let code = base.join("downloads/payroll-service");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&code).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);
    let seen = Arc::new(Mutex::new(None));
    let seen_worker = seen.clone();
    let code_worker = code.clone();
    let ctx = egui::Context::default();
    let mut app = KooladeApp {
        clone_computation_override: Some(Arc::new(move |url, repo| {
            *seen_worker.lock().unwrap() = Some((url, repo));
            Ok(code_worker.clone())
        })),
        ..Default::default()
    };
    app.wizard_open(WelcomeEntry::CloneRepository);
    type_into(
        &mut app,
        &ctx,
        "Example: https://github.com/zorgco/payroll-service",
        "https://github.com/ZorgCo/PayrollService",
    );
    app.wizard_continue();
    app.wizard_commit();
    assert!(app.clone_job.is_some());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let job = app.clone_job.take().expect("clone worker was dispatched");
    while !job.join.is_finished() {
        assert!(
            std::time::Instant::now() < deadline,
            "clone worker timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(job.join.join().unwrap().unwrap(), code);
    assert_eq!(
        seen.lock().unwrap().clone(),
        Some((
            "https://github.com/ZorgCo/PayrollService.git".into(),
            "PayrollService".into()
        ))
    );
    assert!(matches!(app.screen, Screen::Welcome));
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn clone_worker_failure_stays_on_the_wizard_with_a_retry_message() {
    use std::sync::Arc;
    let base = std::env::temp_dir().join(format!(
        "koolade-clone-failure-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let home = base.join("operator");
    std::fs::create_dir_all(&home).unwrap();
    let mut state_home = StateHome::new();
    state_home.set(&home);
    let ctx = egui::Context::default();
    let mut app = KooladeApp {
        clone_computation_override: Some(Arc::new(
            |_url: String, _repo: String| -> Result<std::path::PathBuf, crate::error::AppError> {
                panic!("simulated clone worker failure")
            },
        )),
        ..Default::default()
    };
    app.wizard_open(WelcomeEntry::CloneRepository);
    type_into(
        &mut app,
        &ctx,
        "Example: https://github.com/zorgco/payroll-service",
        "https://github.com/ZorgCo/Fragile",
    );
    app.wizard_continue();
    app.wizard_commit();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while app.clone_job.is_some() {
        app.tick(0.016, &ctx);
        assert!(
            std::time::Instant::now() < deadline,
            "clone worker timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    assert!(matches!(app.screen, Screen::Welcome));
    assert!(app.wiz.is_some(), "failed setup keeps the wizard open");
    let visible = visible_text(&super::super::support::frame(&mut app, &ctx, Vec::new()));
    assert!(
        visible
            .iter()
            .any(|line| line.contains("stopped unexpectedly")),
        "worker error should remain visible for recovery: {visible:?}"
    );
    assert!(
        visible.iter().any(|line| line.contains("retry")),
        "worker error should explain retry: {visible:?}"
    );
    assert!(app.clone_job.is_none());
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}
