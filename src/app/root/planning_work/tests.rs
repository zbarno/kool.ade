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
fn follow_up_command_persists_the_cycle_before_starting_its_scoped_conversation() {
    // Production boundary: the TaskIntent Details command mutates the shared
    // planning store and starts a cycle-specific Manager conversation. The
    // store reload and task-turn assertion cover more than the cycle model's
    // unit test alone. Uses only a local temporary application store and fixture
    // harness; setup cost is low.
    let mut app = crate::app::root::board_tests::fixture();
    let Screen::Connected(project) = &mut app.screen else {
        panic!("fixture must be connected")
    };
    let store = project.state.planning_store.clone();
    let mut intent = Work::new(
        String::new(),
        "Original request".into(),
        "Keep the first request intact".into(),
        "Original work finished".into(),
    );
    intent.kind = WorkKind::Question;
    intent.status = crate::core::planning_work::WorkStatus::Done;
    intent.key = format!("task:{}", intent.uid);
    intent.task_intent_uid = Some(intent.uid.clone());
    let root_uid = intent.uid.clone();
    let root_key = intent.key.clone();
    project.planning_work.push(intent);
    project.save_planning_work().unwrap();
    app.task_harness = Some(Box::new(FixtureHarness));

    app.start_task_intent_follow_up(&root_uid, "Answer a new follow-up question");

    let Screen::Connected(project) = &app.screen else {
        panic!("cycle start keeps the workspace connected")
    };
    let root_work = project
        .planning_work
        .iter()
        .find(|work| work.uid == root_uid)
        .unwrap();
    assert_eq!(
        root_work.original_goal.as_deref(),
        Some("Keep the first request intact")
    );
    let cycle = root_work.task_intent_cycles.last().unwrap();
    let cycle_work = project
        .planning_work
        .iter()
        .find(|work| work.uid == cycle.work_uid)
        .unwrap();
    assert_eq!(cycle.sequence, 2);
    assert_eq!(cycle_work.parent_uid.as_deref(), Some(root_uid.as_str()));
    assert_ne!(cycle_work.key, root_key);
    let cycle_key = cycle_work.key.clone();
    assert!(project.task_turns.contains_key(&cycle_key));
    let persisted = crate::core::planning_work::load(&store).unwrap();
    assert!(persisted.iter().any(|work| work.uid == cycle_work.uid));
    if let Screen::Connected(project) = &mut app.screen
        && let Some(turn) = project.task_turns.remove(&cycle_key)
    {
        turn.request_cancel();
    }
}
