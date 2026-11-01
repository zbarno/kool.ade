use super::*;

#[test]
/// Production boundary: new planning tasks run in independent task streams
/// while Main Chat remains active, and each result updates only its own board
/// record. A unit test cannot cover task creation, worker polling, and adoption
/// together; setup uses temporary local Git repositories and no Docker/network.
fn multiple_new_planning_tasks_keep_their_own_streams_while_main_chat_runs() {
    let root = std::env::temp_dir().join(format!(
        "koolade-parallel-planning-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "--quiet", "-b", "main"],
        vec!["config", "user.name", "Koolade Parallel Test"],
        vec!["config", "user.email", "koolade-parallel@example.invalid"],
    ] {
        assert!(
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut app = fixture();
    if let Screen::Connected(project) = &mut app.screen {
        project.state = crate::core::state::PlannerState::load(&root).unwrap();
        project.state.bootstrap_missing().unwrap();
        project.refresh_git();
    }
    app.task_harness = Some(Box::new(StoppedHarness {
        wait_for_cancel: true,
    }));
    app.start_turn("Keep the main planning stream active");
    let main_key = match &app.screen {
        Screen::Connected(project) => project.active_planning_work.clone().unwrap(),
        _ => panic!("project remains connected"),
    };

    let mut task_keys = Vec::new();
    for description in ["Plan export feature", "Investigate login bug"] {
        app.task_harness = Some(Box::new(StoppedHarness {
            wait_for_cancel: true,
        }));
        app.create_planning_task(
            if description.contains("bug") {
                crate::core::planning_work::WorkKind::Bug
            } else {
                crate::core::planning_work::WorkKind::Feature
            },
            description,
            None,
            None,
            None,
            Default::default(),
        );
        let Screen::Connected(project) = &app.screen else {
            panic!("project remains connected")
        };
        let key = project.planning_work.last().unwrap().key.clone();
        assert!(project.task_turns.contains_key(&key));
        assert!(project.active_turn.is_some());
        assert_eq!(
            project.active_planning_work.as_deref(),
            Some(main_key.as_str())
        );
        task_keys.push(key);
    }

    for key in &task_keys {
        app.cancel_task_reply(key);
    }
    complete(&mut app);
    complete(&mut app);

    let Screen::Connected(project) = &app.screen else {
        panic!("project remains connected")
    };
    assert!(
        project.active_turn.is_some(),
        "main turn survives both task results"
    );
    assert_eq!(
        project.active_planning_work.as_deref(),
        Some(main_key.as_str())
    );
    assert_eq!(
        project
            .planning_work
            .iter()
            .find(|work| work.key == main_key)
            .unwrap()
            .status,
        crate::core::planning_work::WorkStatus::InProgress
    );
    for key in &task_keys {
        assert_eq!(
            project
                .planning_work
                .iter()
                .find(|work| &work.key == key)
                .unwrap()
                .status,
            crate::core::planning_work::WorkStatus::NeedsAttention
        );
    }
    project.active_turn.as_ref().unwrap().request_cancel();
    complete(&mut app);
    std::fs::remove_dir_all(root).unwrap();
}
