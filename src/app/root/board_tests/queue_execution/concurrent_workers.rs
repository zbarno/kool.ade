use super::super::*;
use std::os::unix::fs::PermissionsExt;

#[path = "concurrent_workers/fake_pi.rs"]
mod fake_pi;

#[test]
fn auto_queue_runs_independent_tasks_past_review_and_during_planning() {
    let _shield = crate::core::gitops::test_support::shield("auto-queue-e2e");
    struct Restore(Option<std::ffi::OsString>);
    impl Drop for Restore {
        fn drop(&mut self) {
            unsafe {
                match self.0.take() {
                    Some(value) => std::env::set_var("KOOLADE_PI_BIN", value),
                    None => std::env::remove_var("KOOLADE_PI_BIN"),
                }
            }
        }
    }
    let _restore = Restore(std::env::var_os("KOOLADE_PI_BIN"));
    struct RestorePath(Option<std::ffi::OsString>);
    impl Drop for RestorePath {
        fn drop(&mut self) {
            unsafe {
                match self.0.take() {
                    Some(value) => std::env::set_var("PATH", value),
                    None => std::env::remove_var("PATH"),
                }
            }
        }
    }
    let _restore_path = RestorePath(std::env::var_os("PATH"));
    let root = std::env::temp_dir().join(format!(
        "koolade-auto-e2e-{}",
        chrono::Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let repo = root.join("repo");
    let remote = root.join("remote.git");
    std::fs::create_dir_all(repo.join(".koolade-packet/planning/tasks/fixture")).unwrap();
    std::fs::create_dir_all(repo.join(".koolade-packet/state")).unwrap();
    let git = |cwd: &std::path::Path, args: &[&str]| {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "Fixture"]);
    git(&repo, &["config", "user.email", "fixture@example.test"]);
    let docs = (1..=4)
        .map(|number| {
            let dependencies = if number == 3 {
                "- [Task 001](001-task.md) must be complete.\n- [Task 002](002-task.md) must be complete."
            } else {
                "None."
            };
            crate::artifacts::task_docs::TaskDocument {
                path: format!(".koolade-packet/planning/tasks/fixture/{number:03}-task.md"),
                title: format!("Task {number}"),
                text: format!(
                    "# Task {number}\n\n## Dependencies\n{dependencies}\n\n## Affected files and components\n- src/task-{number}.rs\n\n## Acceptance criteria\n- File exists.\n"
                ),
                identity: None,
                metadata: None,
                metadata_error: None,
            }
        })
        .collect::<Vec<_>>();
    for doc in &docs {
        std::fs::create_dir_all(repo.join(&doc.path).parent().unwrap()).unwrap();
        std::fs::write(repo.join(&doc.path), &doc.text).unwrap();
    }
    let feature = "# CHG-001: Fixture\n\n**Status:** Ready\n\n## Intent\nFixture.\n\n## Current Behavior\nFixture.\n\n## Desired Behavior\nFixture.\n\n## Scope\nFixture.\n\n## Affected Product Areas\nFixture.\n\n## Requirements\nFixture.\n\n## Decisions and Assumptions\nFixture.\n\n## Acceptance Criteria\nFixture.\n";
    std::fs::create_dir_all(repo.join(".koolade-packet/planning/changes/CHG-001-fixture")).unwrap();
    std::fs::write(
        repo.join(".koolade-packet/planning/changes/CHG-001-fixture/specification.md"),
        feature,
    )
    .unwrap();
    std::fs::write(repo.join(".koolade-packet/state/workflow.json"), serde_json::json!({"brief":null,"reviewedSpecification":null,"taskBatches":[{"feature":"fixture","directory":".koolade-packet/planning/tasks/fixture","count":4}]}).to_string()).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "baseline"]);
    git(&root, &["init", "--bare", "-q", remote.to_str().unwrap()]);
    let github_remote = "https://github.com/koolade-fixture/fixture.git";
    git(&repo, &["remote", "add", "origin", github_remote]);
    git(
        &repo,
        &[
            "config",
            &format!("url.{}.insteadOf", remote.display()),
            github_remote,
        ],
    );
    git(&repo, &["push", "-q", "origin", "main"]);
    let fake_bin = root.join("fake-bin");
    std::fs::create_dir_all(&fake_bin).unwrap();
    let fake_gh = fake_bin.join("gh");
    std::fs::write(
            &fake_gh,
            "#!/bin/sh\ncommit=''\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = --commit ]; then shift; commit=$1; fi\n  shift\ndone\nprintf '[{\"headSha\":\"%s\",\"status\":\"completed\",\"conclusion\":\"success\",\"workflowName\":\"fixture\",\"createdAt\":\"2099-01-01T00:00:00Z\",\"url\":\"https://github.com/koolade-fixture/fixture/actions/runs/1\"}]\\n' \"$commit\"\n",
        )
        .unwrap();
    std::fs::set_permissions(&fake_gh, std::fs::Permissions::from_mode(0o700)).unwrap();
    let test_path = std::env::join_paths(std::iter::once(fake_bin.clone()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    unsafe { std::env::set_var("PATH", test_path) };
    // Planning approval commonly exists only in the planning-root checkout
    // when Auto starts. Its commit must remain an ancestor of published work.
    std::fs::write(
        repo.join(".koolade-packet/planning/local-approval.md"),
        "approved locally\n",
    )
    .unwrap();
    git(
        &repo,
        &["add", ".koolade-packet/planning/local-approval.md"],
    );
    git(&repo, &["commit", "-qm", "approve local plan"]);
    let pi = fake_pi::create(&root);
    unsafe { std::env::set_var("KOOLADE_PI_BIN", &pi) };
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&repo).unwrap();
        project.task_documents = docs.clone();
        project.queue.max_parallel = 2;
        project.queue.auto_publish = true;
        project.queue.blocked.insert(
                docs[1].path.clone(),
                crate::core::implementation::Failure::new(
                    crate::core::implementation::FailureKind::RemoteDiverged,
                    crate::core::implementation::RecoveryDisposition::AutomaticRetry,
                    "Local main and freshly fetched origin/main diverged before publication; verified work is preserved",
                ),
            );
        project.implementation_states.clear();
        project.chat_slug = format!("auto-e2e-{}", std::process::id());
    }
    // Exercise the board task action, including worker dispatch,
    // concurrent execution and integration into the remote.
    if let Screen::Connected(project) = &mut app.screen {
        project.state.active_feature = Some(("CHG-001".into(), feature.into()));
        assert!(!crate::core::workflow::feature_approved(
            &repo,
            &project.state.workflow,
            "CHG-001"
        ));
    }
    // Feature approval is a separate explicit action in Specifications;
    // implementation then starts from the selected task's board details.
    app.approve_feature_only("CHG-001");
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "Task 1");
    click_text(&mut app, &ctx, "Implement & continue queue");
    if let Screen::Connected(project) = &app.screen {
        assert!(project.active_turn.is_none());
        assert!(project.active_implementations.contains_key(&docs[0].path));
        assert!(crate::core::workflow::feature_approved(
            &repo,
            &project.state.workflow,
            "CHG-001"
        ));
        let saved = crate::core::state::PlannerState::load(&repo).unwrap();
        assert!(crate::core::workflow::feature_approved(
            &repo,
            &saved.workflow,
            "CHG-001"
        ));
    }
    let ctx = egui::Context::default();
    let deadline = Instant::now() + Duration::from_secs(80);
    let mut concurrent_chat = false;
    let mut planning_and_queued_task_overlap = false;
    let mut saw_capacity_wait = false;
    let mut max_workers = 0;
    loop {
        app.tick(0.1, &ctx);
        max_workers = max_workers.max(app.active_task_count());
        assert!(app.active_task_count() <= 2);
        if !concurrent_chat
            && matches!(&app.screen, Screen::Connected(p) if p.active_implementations.len() == 2)
        {
            app.start_turn("Can we discuss planning while the task runs?");
            assert!(app.conversation_busy());
            concurrent_chat = true;
        }
        if matches!(&app.screen, Screen::Connected(project) if project.active_turn.is_some() && project.active_implementations.contains_key(&docs[3].path))
        {
            planning_and_queued_task_overlap = true;
        }
        if matches!(&app.screen, Screen::Connected(project) if project.active_implementations.len() == 2 && !project.active_implementations.contains_key(&docs[3].path) && project.queue.waiting_for_capacity.contains(&docs[3].path))
        {
            assert!(
                app.queue_status()
                    .contains("queued while all implementation slots are occupied")
            );
            let board = frame(&mut app, &ctx, vec![]);
            assert!(text_contains(
                &board,
                "Queued · implementation slots are full"
            ));
            saw_capacity_wait = true;
        }
        let finished = matches!(&app.screen, Screen::Connected(project) if !project.queue.running && project.active_implementations.is_empty() && project.active_turn.is_none());
        if finished {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "queue did not finish: {}",
            app.queue_status()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(max_workers, 2, "Independent implementations must overlap");
        assert_eq!(project.queue.recovery_attempts.get(&docs[1].path), Some(&1));
        for doc in &docs[..2] {
            assert!(!project.queue.blocked.contains_key(&doc.path));
            assert_eq!(
                project.implementation_states.get(&doc.path).unwrap().status,
                ImplementationStatus::AwaitingApproval
            );
        }
        assert!(!project.implementation_states.contains_key(&docs[2].path));
        assert_eq!(
            project
                .implementation_states
                .get(&docs[3].path)
                .unwrap()
                .status,
            ImplementationStatus::AwaitingApproval,
            "an unrelated task must continue after other tasks reach an approval gate"
        );
        assert!(
            project
                .queue
                .last_error
                .contains("waiting for you to publish")
        );
        assert!(project.queue.last_error.contains("waiting for dependency"));
    }
    assert!(
        concurrent_chat,
        "Planning should start while implementations are running"
    );
    assert!(
        planning_and_queued_task_overlap,
        "The queue must start unrelated work while planning is active"
    );
    assert!(
        saw_capacity_wait,
        "ready work should remain queued while all worker slots are occupied"
    );
    assert!(
        app.chat_messages()
            .iter()
            .any(|m| m.text == "Planning fixture: I can discuss this while the worker runs.")
    );
    for doc in &docs[..2] {
        let progress = crate::core::implementation::load_activity(&repo, &doc.path).unwrap();
        assert!(
            progress.activity.is_some()
                || !progress.posts.is_empty()
                || !progress.response.is_empty()
        );
        assert!(!progress.response.contains("Manager fixture"));
        assert!(!progress.response.contains("Planning fixture"));
    }
    let published_files = git(&remote, &["ls-tree", "-r", "--name-only", "main"]);
    assert_eq!(git(&remote, &["rev-list", "--count", "main"]), "1");
    assert!(!published_files.contains(".koolade-packet/planning/local-approval.md"));
    assert_eq!(
        std::fs::read_to_string(repo.join(".koolade-packet/planning/local-approval.md")).unwrap(),
        "approved locally\n"
    );
    assert!(!published_files.contains("001-task.txt"));
    assert!(!published_files.contains("002-task.txt"));
    assert!(!published_files.contains("003-task.txt"));
    assert!(
        !crate::core::implementation_queue::Queue::load(&repo)
            .unwrap()
            .running
    );
    drop(app);
    std::fs::remove_dir_all(&root).unwrap();
}
