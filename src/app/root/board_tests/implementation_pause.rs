use super::*;
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

struct ResumeHarness(mpsc::Sender<()>);

impl crate::harness::AiHarness for ResumeHarness {
    fn label(&self) -> String {
        "paused task resume fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::AppError> {
        let _ = self.0.send(());
        Err(crate::AppError::Other(
            "resume dispatch reached the fixture".into(),
        ))
    }
}

/// Production boundary: user pause -> failed controller poll -> persisted queue
/// checkpoint -> Task Details Resume -> normal ImplementationController.
/// The invariant is that a paused error remains explicitly resumable and the
/// same task reaches a fresh controller. Unit tests cannot prove the app command
/// and controller startup consume the persisted queue state.
/// Requires opt-in rootless Docker readiness and the approved runtime image;
/// no container command, model service, or public network is used. Setup is one
/// short application flow.
#[test]
fn failed_paused_task_persists_resume_marker_and_restarts_from_task_details() {
    let temp = tempfile::tempdir_in("/tmp").unwrap();
    let mut environment = crate::test_support::ProcessEnvironmentGuard::new();
    environment.set("KOOLADE_HOME", temp.path().join("koolade-home"));
    let root = temp.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success(),
            "git {args:?} failed"
        );
    };
    git(&["init", "-q", "-b", "main"]);
    git(&["config", "user.name", "Koolade Test"]);
    git(&["config", "user.email", "koolade@example.test"]);
    let ticket = "planning/tasks/fixture/001-task.md";
    let ticket_text = "# First task\n\nUnique story detail 0\n";
    let ticket_path = root.join(ticket);
    std::fs::create_dir_all(ticket_path.parent().unwrap()).unwrap();
    std::fs::write(&ticket_path, ticket_text).unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "fixture task"]);
    let base_commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(base_commit.status.success());
    let base_commit = String::from_utf8(base_commit.stdout)
        .unwrap()
        .trim()
        .to_owned();
    let allocation = crate::core::implementation::key_for_ticket(ticket);
    let branch = format!("koolade/{allocation}");
    let worktree = root
        .parent()
        .unwrap()
        .join(".koolade-worktrees")
        .join(crate::persistence::project_slug(
            &root.canonicalize().unwrap(),
        ))
        .join(&allocation);
    std::fs::create_dir_all(worktree.parent().unwrap()).unwrap();
    git(&[
        "worktree",
        "add",
        "-b",
        &branch,
        worktree.to_str().unwrap(),
        "HEAD",
    ]);

    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
        let mut record = project
            .implementation_states
            .values()
            .next()
            .unwrap()
            .clone();
        record.ticket = ticket.into();
        record.ticket_text = ticket_text.into();
        record.branch = branch;
        record.task_repository_allocation_key = Some(allocation);
        record.task_repository = worktree.clone();
        record.task_repositories = vec![worktree];
        record.base_commit = base_commit;
        record.verified_head = None;
        record.status = ImplementationStatus::Blocked;
        record.pr_url = None;
        record.detail = "preserved failure before pause".into();
        let state_dir = crate::core::implementation::state_dir(&root, ticket).unwrap();
        crate::core::implementation::save(&state_dir, &record).unwrap();
        project.implementation_states.insert(ticket.into(), record);
        project.queue.running = true;
        project.queue.recovery_attempts.insert(ticket.into(), 1);
        project.active_implementations.insert(
            ticket.into(),
            crate::core::implementation::Controller::failed_fixture(
                crate::core::implementation::Failure::new(
                    crate::core::implementation::FailureKind::Other,
                    crate::core::implementation::RecoveryDisposition::ExplicitResume,
                    "controller stopped after pause request",
                ),
            ),
        );
    }

    app.pause_task_for(ticket);
    crate::app::root::tick::poll_implementations_for_test(&mut app);
    {
        let Screen::Connected(project) = &app.screen else {
            panic!("disconnected")
        };
        assert!(!project.active_implementations.contains_key(ticket));
        assert!(project.queue.task_paused(ticket));
        assert!(!project.queue.recovery_attempts.contains_key(ticket));
        let persisted = crate::core::implementation_queue::Queue::load(&root).unwrap();
        assert!(persisted.task_paused(ticket));
        assert!(
            persisted
                .recoverable_tickets(&project.task_documents)
                .is_empty()
        );
    }

    if std::env::var("KOOLADE_DOCKER_BACKEND_INTEGRATION").as_deref() != Ok("1") {
        eprintln!("skipping controller resume; set KOOLADE_DOCKER_BACKEND_INTEGRATION=1");
        drop(app);
        return;
    }

    let (resume_tx, resume_rx) = mpsc::channel();
    app.task_harness = Some(Box::new(ResumeHarness(resume_tx)));
    crate::harness::runtime_capabilities::RuntimeCapabilities::invalidate();
    app.dispatch_ui_command(crate::ui::ApplicationCommand::TaskDetail(
        crate::ui::task_detail::Command::StartOrResume {
            ticket: ticket.into(),
        },
    ));
    let Screen::Connected(project) = &app.screen else {
        panic!("disconnected")
    };
    assert!(
        project.active_implementations.contains_key(ticket),
        "Resume did not start a controller: {}",
        project.queue.last_error
    );
    assert!(!project.queue.task_paused(ticket));
    assert!(!project.queue.blocked.contains_key(ticket));
    assert!(project.queue.in_flight.contains(ticket));
    resume_rx
        .recv_timeout(Duration::from_secs(30))
        .expect("resumed controller reaches the configured harness");
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        crate::app::root::tick::poll_implementations_for_test(&mut app);
        if matches!(&app.screen, Screen::Connected(project)
            if !project.active_implementations.contains_key(ticket))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "resumed controller did not finish its fixture failure"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
