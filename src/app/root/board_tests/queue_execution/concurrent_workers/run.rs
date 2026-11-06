use super::super::super::*;

#[path = "run/diagnostics.rs"]
mod diagnostics;
use super::setup::{self, Prepared};
use diagnostics::execution_lock_diagnostics;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub(super) fn exercise(prepared: Prepared) -> PathBuf {
    let Prepared {
        root,
        repo_root,
        local_state_root,
        remote,
        docs,
        mut app,
    } = prepared;
    let ctx = egui::Context::default();
    // Hosted nextest runs this UI loop alongside CPU and Docker-heavy tests.
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut concurrent_chat = false;
    let mut planning_and_queued_task_overlap = false;
    let mut saw_capacity_wait = false;
    let mut task_four_queued = false;
    let mut max_workers = 0;
    loop {
        app.tick(0.1, &ctx);
        max_workers = max_workers.max(app.active_task_count());
        assert!(app.active_task_count() <= 2);
        let first_two_active = matches!(
            &app.screen,
            Screen::Connected(project)
                if project.active_implementations.contains_key(&docs[0].path)
                    && project.active_implementations.contains_key(&docs[1].path)
        );
        if !task_four_queued && first_two_active {
            // Add the capacity waiter only after both slots are occupied. This
            // makes the queue assertion independent of retry scheduling order.
            if let Screen::Connected(project) = &mut app.screen {
                project.task_documents.push(docs[3].clone());
                task_four_queued = true;
            }
        }
        let task_four_active = matches!(
            &app.screen,
            Screen::Connected(project)
                if project.active_implementations.contains_key(&docs[3].path)
        );
        let planning_turn_available = matches!(
            &app.screen,
            Screen::Connected(project) if project.active_turn.is_none()
        );
        if !concurrent_chat
            && planning_turn_available
            && task_four_active
            && root.join("004-task.started").exists()
        {
            app.start_turn("Can we discuss planning while the task runs?");
            assert!(app.conversation_busy());
            concurrent_chat = true;
        }
        if !planning_and_queued_task_overlap
            && task_four_active
            && root.join("004-task.started").exists()
            && root.join("planning.started").exists()
            && concurrent_chat
        {
            planning_and_queued_task_overlap = true;
            std::fs::write(root.join("planning-overlap-observed"), "").unwrap();
        }
        if !saw_capacity_wait
            && matches!(&app.screen, Screen::Connected(project) if project.active_implementations.len() == 2 && !project.active_implementations.contains_key(&docs[3].path) && project.queue.waiting_for_capacity.contains(&docs[3].path))
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
            std::fs::write(root.join("capacity-wait-observed"), "")
                .expect("release the two fixture workers after observing queued capacity");
        }
        let finished = matches!(&app.screen, Screen::Connected(project) if !project.queue.running && project.active_implementations.is_empty() && project.active_turn.is_none());
        if finished {
            break;
        }
        if Instant::now() >= deadline {
            let progress = match &app.screen {
                Screen::Connected(project) => format!(
                    "active={:?}, planning_turn_active={}, states={:?}, waiting={:?}, last_error={}",
                    project.active_implementations.keys().collect::<Vec<_>>(),
                    project.active_turn.is_some(),
                    project
                        .implementation_states
                        .iter()
                        .map(|(path, state)| (path, state.status, &state.detail))
                        .collect::<Vec<_>>(),
                    project.queue.waiting_for_capacity,
                    project.queue.last_error,
                ),
                _ => "project disconnected".into(),
            };
            let provider_trace = std::fs::read_to_string(root.join("fake-pi-trace.jsonl"))
                .unwrap_or_else(|error| format!("trace unavailable: {error}"));
            let fixture_progress = [
                "capacity-wait-observed",
                "mcp-started-task-1",
                "mcp-tool-call-started-task-1",
                "mcp-tool-call-returned-task-1",
                "mcp-stop-started-task-1",
                "mcp-stop-finished-task-1",
                "capacity-wait-entered-task-1",
                "capacity-wait-released-task-1",
                "mcp-started-task-2",
                "mcp-tool-call-started-task-2",
                "mcp-tool-call-returned-task-2",
                "mcp-stop-started-task-2",
                "mcp-stop-finished-task-2",
                "capacity-wait-entered-task-2",
                "capacity-wait-released-task-2",
            ]
            .map(|marker| (marker, root.join(marker).exists()));
            panic!(
                "queue did not finish: {}; {progress}; fixture_progress={fixture_progress:?}; provider_trace={provider_trace}",
                app.queue_status()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Screen::Connected(project) = &app.screen {
        assert_eq!(
            max_workers,
            2,
            "Independent implementations must overlap; final_statuses={:?}, queue_running={}",
            project
                .implementation_states
                .iter()
                .map(|(path, state)| (path, state.status))
                .collect::<Vec<_>>(),
            project.queue.running
        );
        assert_eq!(project.queue.recovery_attempts.get(&docs[1].path), Some(&1));
        for doc in &docs[..2] {
            let task_uid = doc
                .identity
                .as_ref()
                .expect("fixture task has an identity")
                .uid
                .as_str();
            assert!(
                !project.queue.blocked.contains_key(&doc.path),
                "task {} remained blocked: {:?}; execution lock diagnostics={}; fake Pi trace={}",
                doc.path,
                project.queue.blocked.get(&doc.path),
                execution_lock_diagnostics(&root, task_uid),
                std::fs::read_to_string(root.join("fake-pi-trace.jsonl"))
                    .unwrap_or_else(|error| format!("trace unavailable: {error}"))
            );
            let implementation = project.implementation_states.get(&doc.path).unwrap_or_else(
                || {
                    panic!(
                        "completed fixture task has no persisted implementation state; known={:?}",
                        project.implementation_states.keys().collect::<Vec<_>>()
                    )
                },
            );
            assert_eq!(
                implementation.status,
                ImplementationStatus::AwaitingApproval
            );
        }
        assert!(!project.implementation_states.contains_key(&docs[2].path));
        let unrelated = project
            .implementation_states
            .get(&docs[3].path)
            .unwrap_or_else(|| {
                let excluded = project.queue.blocked.keys().cloned().collect();
                let running = project
                    .active_implementations
                    .keys()
                    .cloned()
                    .collect();
                let scheduler = crate::core::implementation_queue::next_ready_ticket_with_lifecycle_subtasks(
                    &project.task_documents,
                    &project.implementation_states,
                    &excluded,
                    &running,
                    &std::collections::BTreeSet::new(),
                );
                panic!(
                    "unrelated task did not start after capacity opened; queue_error={:?}, waiting={:?}, blocked={:?}, scheduler={scheduler:?}, task_paths={:?}",
                    project.queue.last_error,
                    project.queue.waiting_for_capacity,
                    project.queue.blocked.keys().collect::<Vec<_>>(),
                    project.task_documents.iter().map(|doc| &doc.path).collect::<Vec<_>>()
                )
            });
        assert_eq!(
            unrelated.status,
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
        let progress = match &app.screen {
            Screen::Connected(project) => {
                let task_uid = project
                    .implementation_states
                    .get(&doc.path)
                    .and_then(|state| state.task_uid.as_deref());
                crate::core::implementation::load_activity_for_task(&repo_root, &doc.path, task_uid)
                    .unwrap()
            }
            _ => panic!("the project should remain connected after worker completion"),
        };
        assert!(
            progress.activity.is_some()
                || !progress.posts.is_empty()
                || !progress.response.is_empty()
        );
        assert!(!progress.response.contains("Manager fixture"));
        assert!(!progress.response.contains("Planning fixture"));
    }
    let published_files = setup::git(&remote, &["ls-tree", "-r", "--name-only", "main"]);
    assert_eq!(setup::git(&remote, &["rev-list", "--count", "main"]), "1");
    assert!(!published_files.contains("planning/local-approval.md"));
    assert_eq!(
        std::fs::read_to_string(local_state_root.join("planning/local-approval.md")).unwrap(),
        "approved locally\n"
    );
    assert!(!published_files.contains("001-task.txt"));
    assert!(!published_files.contains("002-task.txt"));
    assert!(!published_files.contains("003-task.txt"));
    assert!(
        !crate::core::implementation_queue::Queue::load(&local_state_root)
            .unwrap()
            .running
    );
    drop(app);
    root
}
