#[path = "app_controller/harness.rs"]
mod harness;
#[path = "app_controller/revocation_retry.rs"]
mod revocation_retry;
use crate::{
    app::root::Screen, core::self_improvement::SelfImprovementStore,
    execution::contract::is_valid_sha,
};
use harness::ControllerDockerHarness;
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

/// Production boundary: Pursue-approved task -> normal ImplementationController
/// -> isolated task clone -> app-issued authorization -> rootless Docker command.
/// The invariant is that an ordinary controller start supplies the approved
/// task's workspace fence to execution, and app cancellation revokes the next
/// command. Unit/store tests cannot prove the controller-to-session handoff.
/// Requires Linux rootless Docker and the pinned Rust image; setup uses one
/// approved headless app flow and two small Docker commands, with no real model
/// service or public network dependency.
#[test]
fn app_controller_dispatches_approved_task_to_isolated_docker_and_revokes_it_on_cancel() {
    let temp = tempfile::tempdir_in("/tmp").unwrap();
    let test_tmp = temp.path().join("tmp");
    std::fs::create_dir_all(&test_tmp).unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("TMPDIR", &test_tmp);
    environment.set("KOOLADE_HOME", temp.path().join("operator-home"));

    let gh = fake_gh(temp.path());

    let mut ready = super::setup::ready_task(&gh);
    if std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1") {
        assert_eq!(
            ready.run.phase,
            crate::core::self_improvement::SelfImprovementPhaseV1::Ready
        );
        eprintln!(
            "skipping Evolve controller Docker commands; set KOOLADE_DOCKER_BACKEND_INTEGRATION=1"
        );
        return;
    }

    let engine = std::process::Command::new("docker")
        .args(["info", "--format", "{{json .SecurityOptions}}"])
        .output()
        .expect("Docker CLI is available in the rootless integration job");
    assert!(
        engine.status.success()
            && String::from_utf8_lossy(&engine.stdout).contains("name=rootless"),
        "Evolve controller dispatch requires rootless Docker: {}",
        String::from_utf8_lossy(&engine.stderr)
    );
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").expect("rootless Docker runtime dir");
    let docker_host = format!("unix://{runtime_dir}/docker.sock");
    assert!(std::env::var_os("DOCKER_CONTEXT").is_none());
    assert_eq!(
        std::env::var("DOCKER_HOST").as_deref(),
        Ok(docker_host.as_str())
    );

    // Keep the canonical repository identity while resolving Git transport to
    // this deterministic fixture instead of reaching the public network.
    let git_config = temp.path().join("gitconfig");
    std::fs::write(
        &git_config,
        format!(
            "[url \"file://{}\"]\n\tinsteadOf = https://github.com/zbarno/kool.ade\n",
            ready.repo_root.display()
        ),
    )
    .unwrap();
    environment.set("GIT_CONFIG_GLOBAL", &git_config);

    let (initial_tx, initial_rx) = mpsc::channel();
    let (revoked_tx, revoked_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let ticket = ready.ticket.clone();
    let task_uid = ready.metadata.uid.clone();
    let project_id = ready.planning.project_id.hyphenated().to_string();
    let run_uid = ready.run.run_uid.clone();
    let source_repo = ready.repo_root.canonicalize().unwrap();
    let planning = ready.planning.clone();
    ready._app.task_harness = Some(Box::new(ControllerDockerHarness {
        initial: initial_tx,
        revoked: revoked_tx,
        resume: Mutex::new(resume_rx),
    }));
    ready._app.start_implementation_with_capabilities(
        ticket.clone(),
        true,
        crate::harness::runtime_capabilities::RuntimeCapabilities {
            planning_access:
                crate::harness::runtime_capabilities::PlanningAccess::SandboxedRepositoryReads,
            implementation: true,
        },
    );
    let started = match &ready._app.screen {
        Screen::Connected(project) => project.active_implementations.contains_key(&ticket),
        Screen::Welcome => false,
    };
    assert!(
        started,
        "approved task did not enter the normal implementation controller: {}",
        match &ready._app.screen {
            Screen::Connected(project) => &project.queue.last_error,
            Screen::Welcome => "project disconnected",
        }
    );

    let initial = initial_rx
        .recv_timeout(Duration::from_secs(180))
        .expect("normal controller reaches the configured execution harness");
    assert_eq!(
        initial
            .workspace
            .as_ref()
            .map(|workspace| workspace.task_id.as_str()),
        Some(task_uid.as_str())
    );
    assert_eq!(
        initial
            .workspace
            .as_ref()
            .map(|workspace| workspace.project_id.as_str()),
        Some(project_id.as_str())
    );
    assert!(
        initial
            .workspace
            .as_ref()
            .is_some_and(|workspace| { is_valid_sha(&workspace.source_sha) })
    );
    assert!(
        initial.has_authorization,
        "controller omitted the persisted self-improvement fence"
    );
    assert_ne!(
        initial.repository.canonicalize().unwrap(),
        source_repo,
        "self-improvement must execute from its isolated task clone"
    );
    assert!(
        initial.command_succeeded,
        "app-authorized rootless Docker command failed: {}",
        initial.detail
    );
    assert!(
        initial
            .repository
            .join(".self-improvement-controller-authorized")
            .is_file()
    );

    if let Screen::Connected(project) = &mut ready._app.screen {
        project.state.baseline_planning_revision = planning.revision().unwrap();
    }
    ready
        ._app
        .dispatch_ui_command(crate::ui::ApplicationCommand::CancelWork {
            key: ticket.clone(),
        });
    let runs = SelfImprovementStore::new(&planning);
    let cancelled = runs.load(&run_uid).unwrap();
    assert_eq!(
        cancelled.phase,
        crate::core::self_improvement::SelfImprovementPhaseV1::Cancelled
    );
    resume_tx
        .send(())
        .expect("release the fixture harness after cancellation");
    let revoked = revoked_rx
        .recv_timeout(Duration::from_secs(60))
        .expect("cancelled app task reaches a second authorization check");
    assert!(
        revoked.denied_by_policy,
        "cancelled task was not denied by the authorization fence: {}",
        revoked.detail
    );
    assert!(
        !initial
            .repository
            .join(".self-improvement-controller-denied")
            .exists()
    );

    let context = egui::Context::default();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        ready._app.tick(0.016, &context);
        let active = matches!(
            &ready._app.screen,
            Screen::Connected(project) if project.active_implementations.contains_key(&ticket)
        );
        if !active {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "cancelled implementation controller did not finish"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn fake_gh(root: &Path) -> PathBuf {
    let gh = root.join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\n[ \"$1\" = api ] && [ \"$2\" = user ] || exit 2\nprintf '987654321\\n'\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    gh
}
