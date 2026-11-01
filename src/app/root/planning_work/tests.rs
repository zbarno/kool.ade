use super::{Screen, provider_support_follow_up};
use crate::core::planning_work::{Work, WorkKind};

struct FixtureHarness;

impl crate::harness::AiHarness for FixtureHarness {
    fn label(&self) -> String {
        "planning task fixture".into()
    }

    fn check_available(&self) -> Result<String, crate::error::AppError> {
        Ok(self.label())
    }

    fn execute(
        &self,
        _request: &crate::harness::PlanningRequest,
    ) -> Result<crate::harness::HarnessOutcome, crate::error::AppError> {
        Err(crate::error::AppError::Other("fixture turn stopped".into()))
    }
}

fn work(kind: WorkKind, request: &str) -> Work {
    let mut work = Work::new(
        String::new(),
        "Question".into(),
        request.into(),
        String::new(),
    );
    work.kind = kind;
    work
}

#[test]
fn unsupported_hosted_anthropic_question_gets_an_optional_offer_only() {
    let offer = provider_support_follow_up(&work(
        WorkKind::Question,
        "Why can't Kool.ad/e use hosted Anthropic through Pi?",
    ))
    .unwrap();
    assert!(offer.title.contains("hosted Anthropic"));
    assert!(offer.description.contains("sandbox security boundaries"));
    assert!(
        provider_support_follow_up(&work(
            WorkKind::Question,
            "How does Anthropic compare with local providers?",
        ))
        .is_none()
    );
    assert!(
        provider_support_follow_up(&work(
            WorkKind::Feature,
            "Why can't Kool.ad/e use hosted Anthropic through Pi?",
        ))
        .is_none()
    );
}

#[test]
fn planning_only_workspace_creates_tasks_without_code_branches_and_blocks_stale_generation() {
    let root = std::env::temp_dir().join(format!(
        "koolade-planning-only-task-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let manager = crate::core::managed_workspace::WorkspaceManager::new(root.join("home"));
    let workspace = manager.create("Planning only").unwrap();
    let mut app = crate::app::root::board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    project.state = crate::core::state::PlannerState::load_with_store(
        &workspace.planning_repo,
        &workspace.store,
    )
    .unwrap();
    project.planning_work = crate::core::planning_work::load(&workspace.store).unwrap();
    project.git = crate::core::gitops::snapshot(&workspace.planning_repo);
    project.managed_workspace = Some(workspace.clone());
    app.task_harness = Some(Box::new(FixtureHarness));

    app.create_planning_task(
        WorkKind::Feature,
        "Plan a no-code workspace",
        None,
        None,
        None,
        Default::default(),
    );

    let Screen::Connected(project) = &mut app.screen else {
        panic!("task creation keeps the workspace connected")
    };
    let created = project.planning_work.last().unwrap();
    let task_key = created.key.clone();
    assert_eq!(created.source_branch, None);
    assert_eq!(created.destination_branch, None);
    assert_eq!(
        crate::core::planning_work::load(&workspace.store)
            .unwrap()
            .last()
            .unwrap()
            .source_branch,
        None
    );
    let task_turn = project
        .task_turns
        .remove(&task_key)
        .expect("new planning task runs in its own conversation");
    task_turn.request_cancel();
    project.task_live.remove(&task_key);
    let task_generation_key;
    {
        let Screen::Connected(project) = &mut app.screen else {
            panic!("task generation fixture remains connected")
        };
        assert!(!project.has_selected_code_repository());
        let mut generation = work(WorkKind::TaskGeneration, "Generate tasks for a feature");
        generation.feature_id = Some("F-NO-CODE".into());
        project.planning_work.push(generation);
        project.save_planning_work().unwrap();
        task_generation_key = project.planning_work.last().unwrap().key.clone();
    }
    app.start_task_generation(&task_generation_key, "F-NO-CODE");
    let Screen::Connected(project) = &mut app.screen else {
        panic!("task generation fixture remains connected")
    };
    assert!(
        !project
            .state
            .workflow
            .feature_branch_targets
            .contains_key("F-NO-CODE"),
        "planning-only workspaces must not save code branch targets"
    );
    assert!(
        !project.task_turns.contains_key(&task_generation_key),
        "task generation without an active, ready feature must remain blocked"
    );
    let generation = project
        .planning_work
        .iter()
        .find(|work| work.key == task_generation_key)
        .unwrap();
    assert_eq!(
        generation.status,
        crate::core::planning_work::WorkStatus::NeedsAttention
    );
    assert!(
        generation
            .detail
            .contains("readiness evidence is missing or stale")
    );
    let _ = std::fs::remove_dir_all(root);
}
