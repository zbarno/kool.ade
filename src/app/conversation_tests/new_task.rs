use super::*;
use std::sync::{Arc, Mutex};

#[path = "new_task/branch_selection.rs"]
mod branch_selection;

#[test]
fn feature_bug_and_new_project_tasks_persist_their_kind_and_start_from_the_board() {
    let root = std::env::temp_dir().join(format!(
        "koolade-new-kinds-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "--quiet", "-b", "main"],
        vec!["config", "user.name", "Kool.ad/e Task Test"],
        vec!["config", "user.email", "koolade-task@example.invalid"],
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
    let ctx = egui::Context::default();
    for (label, kind, description) in [
        (
            "Feature",
            crate::core::planning_work::WorkKind::Feature,
            "Export project tasks",
        ),
        (
            "Bug",
            crate::core::planning_work::WorkKind::Bug,
            "Fix wrapped blocker details",
        ),
        (
            "New Project",
            crate::core::planning_work::WorkKind::NewProject,
            "Plan a local notes app",
        ),
        (
            "Refresh Documentation",
            crate::core::planning_work::WorkKind::DocumentationRefresh,
            "Document the repository and triage findings",
        ),
    ] {
        app.task_harness = Some(Box::new(StoppedHarness {
            wait_for_cancel: true,
        }));
        frame(&mut app, &ctx, vec![]);
        click_text(&mut app, &ctx, "+ New Task");
        click_text(&mut app, &ctx, label);
        click_text(&mut app, &ctx, "Describe what you want to do…");
        frame(&mut app, &ctx, vec![egui::Event::Text(description.into())]);
        click_text(&mut app, &ctx, "Create Task");
        let uid = {
            let Screen::Connected(project) = &app.screen else {
                panic!()
            };
            assert!(
                project.active_turn.is_some(),
                "{label} starts planning immediately"
            );
            let created = project.planning_work.last().unwrap();
            assert_eq!(created.kind, kind);
            assert_eq!(created.request, description);
            assert_eq!(created.source_branch.as_deref(), Some("main"));
            assert_eq!(created.destination_branch.as_deref(), Some("main"));
            let persisted = crate::core::planning_work::load(&root).unwrap();
            assert_eq!(persisted.last().unwrap().uid, created.uid);
            assert_eq!(
                persisted.last().unwrap().source_branch.as_deref(),
                Some("main")
            );
            assert_eq!(
                persisted.last().unwrap().destination_branch.as_deref(),
                Some("main")
            );
            created.uid.clone()
        };
        if let Screen::Connected(project) = &app.screen {
            project.active_turn.as_ref().unwrap().request_cancel();
        }
        complete(&mut app);
        assert!(
            crate::core::planning_work::load(&root)
                .unwrap()
                .iter()
                .any(|work| work.uid == uid && work.kind == kind)
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[path = "new_task/question_task.rs"]
mod question_task;

#[test]
fn question_answer_can_offer_a_linked_feature_task_that_starts_only_on_selection() {
    let root = std::env::temp_dir().join(format!(
        "koolade-question-followup-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "Kool.ad/e Follow-up Test"],
        vec!["config", "user.email", "koolade-followup@example.invalid"],
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
    app.task_harness = Some(Box::new(ReplyHarness {
        prompts: Arc::new(Mutex::new(Vec::new())),
        reply: serde_json::json!({
            "schema_version": 2,
            "assistant_message": "Hosted Anthropic is not supported by the current provider relay. Create a Feature task if you want Kool.ad/e to plan that capability.",
            "document_updates": [],
            "open_items_added": [],
            "open_items_updated": [],
            "open_items_resolved": [],
            "next_question_id": null,
            "requested_action": null
        })
        .to_string(),
    }));
    let ctx = egui::Context::default();
    frame(&mut app, &ctx, vec![]);
    click_text(&mut app, &ctx, "+ New Task");
    click_text(&mut app, &ctx, "Question task");
    click_text(&mut app, &ctx, "Describe what you want to do…");
    frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text(
            "Why can't Kool.ad/e use hosted Anthropic through Pi?".into(),
        )],
    );
    click_text(&mut app, &ctx, "Create Task");
    complete(&mut app);

    let (parent_uid, parent_title) = {
        let Screen::Connected(project) = &mut app.screen else {
            panic!()
        };
        let parent = project.planning_work.last_mut().unwrap();
        assert_eq!(parent.kind, crate::core::planning_work::WorkKind::Question);
        assert_eq!(parent.status, crate::core::planning_work::WorkStatus::Done);
        parent.source_branch = Some("release/2.1".into());
        parent.destination_branch = Some("integration".into());
        parent.routing_overrides = std::collections::BTreeMap::from([
            (
                crate::persistence::harness_settings::IMPLEMENTATION.into(),
                crate::persistence::harness_settings::WorkRoute {
                    harness: "codex".into(),
                    model: Some("task-model".into()),
                },
            ),
            (
                crate::persistence::harness_settings::DOCUMENTATION.into(),
                crate::persistence::harness_settings::WorkRoute {
                    harness: "claude".into(),
                    model: None,
                },
            ),
        ]);
        let parent_uid = parent.uid.clone();
        project.git.branches = vec!["main".into(), "release/2.1".into(), "integration".into()];
        project.save_planning_work().unwrap();
        let parent = project
            .planning_work
            .iter()
            .find(|work| work.uid == parent_uid)
            .unwrap();
        let offer = parent.follow_up_task.as_ref().unwrap();
        assert_eq!(offer.title, "Add hosted Anthropic support through Pi");
        (
            parent_uid,
            parent.title.chars().take(32).collect::<String>(),
        )
    };
    let output = click_text(&mut app, &ctx, &parent_title);
    assert!(text_position(&output, "Next step").is_some());
    let output = click_text(&mut app, &ctx, "Related task");
    assert!(text_position(&output, "Create related Feature task").is_some());
    click_text(&mut app, &ctx, "Create related Feature task");
    let Screen::Connected(project) = &app.screen else {
        panic!()
    };
    assert!(
        project.active_turn.is_some(),
        "the selected offer starts planning"
    );
    let child = project.planning_work.last().unwrap();
    assert_eq!(child.kind, crate::core::planning_work::WorkKind::Feature);
    assert_eq!(child.parent_uid.as_deref(), Some(parent_uid.as_str()));
    assert_eq!(
        child.routing_inherited_from.as_deref(),
        Some(parent_uid.as_str())
    );
    assert_eq!(child.routing_overrides.len(), 2);
    assert_eq!(
        child.routing_overrides[crate::persistence::harness_settings::IMPLEMENTATION]
            .model
            .as_deref(),
        Some("task-model")
    );
    assert_eq!(
        child.request,
        "Plan support for hosted Anthropic through Pi while preserving Kool.ad/e's sandbox security boundaries."
    );
    let parent = project
        .planning_work
        .iter()
        .find(|work| work.uid == parent_uid)
        .unwrap();
    assert!(
        parent.follow_up_task.is_none(),
        "the offer cannot be selected twice"
    );
    project.active_turn.as_ref().unwrap().request_cancel();
    complete(&mut app);
    std::fs::remove_dir_all(root).unwrap();
}
