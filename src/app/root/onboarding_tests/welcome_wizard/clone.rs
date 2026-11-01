use super::super::support::{StateHome, type_into, visible_text};
use crate::app::root::{KooladeApp, Screen, WelcomeEntry};
use crate::core::managed_workspace::{
    CodeRepositoryRegistration, PAIR_CODE_REPO_ID, WorkspaceManager,
};
use std::sync::{Arc, Mutex};

/// Boundary: wizard confirmation -> worker thread -> managed pairing ->
/// connection. This crosses the asynchronous handoff that a unit test cannot
/// prove. It uses a local Git fixture and an injected clone result; no network,
/// Docker, or SSH is needed.
#[test]
fn clone_wizard_worker_opens_the_saved_managed_pair() {
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
    git(&code, &["init", "--quiet", "--initial-branch=main"]);
    git(
        &code,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/ZorgCo/PayrollService.git",
        ],
    );
    std::fs::write(code.join("service.toml"), "[server]\n").unwrap();
    git(&code, &["add", "-A"]);
    git(
        &code,
        &[
            "-c",
            "user.name=Fixture Operator",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "--quiet",
            "-m",
            "fixture clone",
        ],
    );
    let manager = WorkspaceManager::from_state_root();
    let workspace = manager.create("Payroll Service").unwrap();
    manager
        .register_code_repository(
            workspace.id,
            CodeRepositoryRegistration {
                id: PAIR_CODE_REPO_ID.into(),
                role: "Primary code".into(),
                remote: Some("https://github.com/ZorgCo/PayrollService.git".into()),
                checkout: code.clone(),
            },
        )
        .unwrap();

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
    while app.clone_job.is_some() {
        app.tick(0.016, &ctx);
        assert!(
            std::time::Instant::now() < deadline,
            "clone worker timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    let Screen::Connected(project) = &app.screen else {
        panic!("worker did not open the managed pair: {:?}", app.conn_error);
    };
    assert_eq!(project.state.repo_root, code);
    assert_ne!(project.state.planning_store.root, code);
    assert_eq!(project.state.planning_store.project_id, workspace.id);
    assert_eq!(
        seen.lock().unwrap().clone(),
        Some((
            "https://github.com/ZorgCo/PayrollService.git".into(),
            "PayrollService".into()
        ))
    );
    assert_eq!(manager.list_local().unwrap().len(), 1);
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
    assert!(
        WorkspaceManager::from_state_root()
            .list_local()
            .unwrap()
            .is_empty()
    );
    drop(app);
    let _ = std::fs::remove_dir_all(base);
}

fn git(root: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
