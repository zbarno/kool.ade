#[cfg(target_os = "linux")]
#[path = "production_dispatch/app_controller.rs"]
mod app_controller;
#[path = "production_dispatch/docker.rs"]
mod docker;
#[path = "production_dispatch/setup.rs"]
mod setup;

/// Production boundary: authenticated Pursue, current plan approval, isolated
/// PlanReviewer, and app task generation produce the persisted authorization
/// consumed by a rootless Docker session. Store tests cannot prove this binding
/// across the app and execution session. Requires Linux rootless Docker and the
/// pinned General and Rust images; setup prepares both image receipts, one
/// headless app flow, one local task clone, and two short Docker commands.
#[cfg(unix)]
#[test]
fn pursued_app_task_executes_in_rootless_docker_and_cancellation_revokes_it() {
    let temp = tempfile::tempdir().unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", temp.path().join("operator-home"));
    let gh = temp.path().join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\n[ \"$1\" = api ] && [ \"$2\" = user ] || exit 2\nprintf '987654321\\n'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    let ready = setup::ready_task(&gh);
    if std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1") {
        assert_eq!(
            ready.run.phase,
            crate::core::self_improvement::SelfImprovementPhaseV1::Ready
        );
        eprintln!("skipping Evolve Docker commands; set KOOLADE_DOCKER_BACKEND_INTEGRATION=1");
        return;
    }
    #[cfg(target_os = "linux")]
    {
        if crate::test_support::locked_images::enabled() {
            crate::test_support::locked_images::prepare_profile_for_home(
                &temp.path().join("operator-home"),
                crate::execution::docker::images::RuntimeProfile::Rust,
            )
            .expect("Evolve fixture must receive approved General and Rust image receipts");
        }
        docker::execute(&ready, temp.path());
    }
    #[cfg(not(target_os = "linux"))]
    eprintln!("skipping Evolve Docker commands: rootless Docker integration requires Linux");
}
