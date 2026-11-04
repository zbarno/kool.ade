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
    std::fs::write(root.join("README.md"), "code checkout fixture\n").unwrap();
    git(&["add", "README.md"]);
    git(&["commit", "-q", "-m", "code baseline"]);
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
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
        let local_state_root = project.state.planning_store.local_root();
        let project_slug =
            crate::persistence::project_slug(&local_state_root.canonicalize().unwrap());
        let worktree = crate::persistence::project_dir(&project_slug)
            .join("task-repositories/root")
            .join(&allocation);
        std::fs::create_dir_all(worktree.parent().unwrap()).unwrap();
        git(&[
            "clone",
            "--quiet",
            "--no-hardlinks",
            root.to_str().unwrap(),
            worktree.to_str().unwrap(),
        ]);
        git(&[
            "-C",
            worktree.to_str().unwrap(),
            "checkout",
            "--quiet",
            "-b",
            &branch,
        ]);
        let task_document = project
            .task_documents
            .iter()
            .find(|document| document.path == ticket)
            .expect("fixture contains the selected task");
        let (repository_cache, repository_identity, push_repository) =
            crate::core::implementation::prepare_repository_cache_for_test(
                &project.state.planning_store,
                &root,
                "root",
            )
            .unwrap();
        let task_path = local_state_root.join(ticket);
        std::fs::create_dir_all(task_path.parent().unwrap()).unwrap();
        let stored = crate::artifacts::task_docs::embed(
            &task_document.text,
            task_document
                .metadata
                .as_ref()
                .expect("task metadata is seeded"),
        )
        .unwrap();
        let identity = serde_json::to_string(
            task_document
                .identity
                .as_ref()
                .expect("task identity is seeded"),
        )
        .unwrap();
        std::fs::write(
            task_path,
            format!("{stored}\n<!-- koolade-artifact-id:v1 {identity} -->\n"),
        )
        .unwrap();
        project
            .queue
            .bind_task_documents(&project.task_documents)
            .unwrap();
        let mut record = project
            .implementation_states
            .values()
            .next()
            .unwrap()
            .clone();
        record.ticket = ticket.into();
        record.task_uid = task_document
            .identity
            .as_ref()
            .map(|identity| identity.uid.clone());
        record.project_id = Some(crate::persistence::project_slug(
            &local_state_root.canonicalize().unwrap(),
        ));
        record.workspace_uid = Some(
            project
                .state
                .planning_store
                .project_id
                .hyphenated()
                .to_string(),
        );
        record.repository_id = Some("root".into());
        record.repository_cache = Some(repository_cache);
        record.repository_identity = Some(repository_identity);
        record.push_repository = push_repository;
        record.ticket_text = ticket_text.into();
        record.source_ref = Some("main".into());
        record.source_commit = Some(base_commit.clone());
        record.branch = branch;
        record.task_repository_allocation_key = Some(allocation);
        record.task_repository = worktree.clone();
        record.task_repository_ready = true;
        record.task_repositories = vec![worktree];
        record.base_commit = base_commit;
        record.verified_head = None;
        record.status = ImplementationStatus::Blocked;
        record.pr_url = None;
        record.detail = "preserved failure before pause".into();
        let state_dir = crate::core::implementation::state_dir(&local_state_root, ticket).unwrap();
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
        let mut persisted =
            crate::core::implementation_queue::Queue::load(&project.state.planning_store.root)
                .unwrap();
        persisted
            .bind_task_documents(&project.task_documents)
            .unwrap();
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
    match resume_rx.recv_timeout(Duration::from_secs(30)) {
        Ok(()) => {}
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            crate::app::root::tick::poll_implementations_for_test(&mut app);
            let failure_class = match &app.screen {
                Screen::Connected(project) => project
                    .queue
                    .blocked
                    .get(ticket)
                    .map(|failure| format!("{:?}/{:?}", failure.kind, failure.recovery))
                    .unwrap_or_else(|| "no typed failure recorded".into()),
                Screen::Welcome => "project disconnected".into(),
            };
            panic!(
                "resumed controller stopped before reaching the configured harness; failure_class={failure_class}"
            );
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("resumed controller did not reach the configured harness before timeout")
        }
    }
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
