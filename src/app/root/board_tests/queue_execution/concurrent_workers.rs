#[path = "concurrent_workers/fake_pi.rs"]
mod fake_pi;
#[path = "concurrent_workers/run.rs"]
mod run;
#[path = "concurrent_workers/setup.rs"]
mod setup;

#[test]
fn auto_queue_runs_independent_tasks_past_review_and_during_planning() {
    // Capability detection resolves Docker through PATH, which this fixture
    // replaces later. Keep the initial host observation outside concurrent
    // PATH and provider override windows too.
    let _environment = crate::test_support::lock_process_environment();
    let _shield = crate::core::gitops::test_support::shield("auto-queue-e2e");
    if !fake_pi::sandbox_available() {
        eprintln!(
            "Skipping queue execution integration: application Docker execution capabilities are unavailable"
        );
        return;
    }
    let _restore_pi = fake_pi::restore_environment_variable("KOOLADE_PI_BIN");
    let _restore_path = fake_pi::restore_environment_variable("PATH");
    let _restore_ssh = fake_pi::restore_environment_variable("GIT_SSH_COMMAND");
    let root = std::env::temp_dir().join(format!(
        "koolade-auto-e2e-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let _restore_home = fake_pi::configure_harness_settings(&root);
    let app_binary = std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(std::path::Path::parent)
        .expect("test binary is in target/debug/deps")
        .join("koolade");
    assert!(
        app_binary.is_file(),
        "application binary must be built for the Docker CI suite"
    );
    let _app_binary_override =
        crate::harness::execution_security::override_test_app_binary(app_binary);

    let prepared = setup::prepare(root.clone());
    let cleanup_root = run::exercise(prepared);
    drop(_restore_home);
    fake_pi::remove_test_root(&cleanup_root);
}
