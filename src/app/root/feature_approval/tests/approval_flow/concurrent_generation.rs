use super::*;

#[test]
fn approved_task_generation_runs_alongside_an_unrelated_implementation() {
    let _shield = crate::core::gitops::test_support::shield("parallel-task-generation");
    let (mut app, root, review) = setup();
    if let Screen::Connected(project) = &mut app.screen {
        project.active_implementations.insert(
            ".koolade-packet/planning/tasks/other-feature/001-running.md".into(),
            crate::core::implementation::Controller::idle_fixture(),
        );
    }
    let (review_harness, calls) = harness(vec![review.to_string()]);
    app.task_harness = Some(review_harness);
    let context = egui::Context::default();
    frame(&mut app, &context, vec![]);
    click_text(&mut app, &context, "Approve CHG-004 and prepare tasks");
    assert!(app.feature_approved("CHG-004"));
    let key = match &app.screen {
        Screen::Connected(project) => project
            .planning_work
            .iter()
            .find(|work| work.kind == crate::core::planning_work::WorkKind::TaskGeneration)
            .map(|work| work.key.clone())
            .expect("approval creates a task-generation card"),
        Screen::Welcome => panic!("project stays connected"),
    };

    app.start_task_generation(&key, "CHG-004");
    assert!(
        matches!(&app.screen, Screen::Connected(project) if project.active_turn.is_some() && !project.active_implementations.is_empty())
    );
    let reviewed = finish(&mut app);
    assert!(reviewed);
    assert!(calls.lock().unwrap()[0].contains("do not ask for approval again"));

    let (story_harness, generated) = harness(vec![
        include_str!("../../../../../../tests/fixtures/task-outline.json").into(),
        compact_story(
            include_str!("../../../../../../tests/fixtures/task-story-1.json"),
            false,
        ),
        compact_story(
            include_str!("../../../../../../tests/fixtures/task-story-2.json"),
            true,
        ),
    ]);
    app.task_harness = Some(story_harness);
    app.continue_feature_generation(reviewed);
    assert!(
        matches!(&app.screen, Screen::Connected(project) if project.active_turn.is_some() && !project.active_implementations.is_empty()),
        "story generation must not wait for unrelated implementation work"
    );
    assert!(finish(&mut app));
    assert_eq!(generated.lock().unwrap().len(), 3);
    if let Screen::Connected(project) = &mut app.screen {
        project.active_implementations.clear();
    }
    assert!(matches!(&app.screen, Screen::Connected(project) if has_current_task_batch(project)));
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
