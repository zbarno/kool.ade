use super::*;
use crate::core::{apply, validation};

fn brief() -> InterviewBrief {
    InterviewBrief {
        feature_name: "Saved searches".into(),
        problem: "Analysts repeat the same query configuration every morning.".into(),
        goal: "Let analysts resume a useful query without reconstructing it.".into(),
        target_users: "Analysts monitoring daily operational changes.".into(),
        intended_outcome: "Select a saved query and see current matching results.".into(),
        success_criteria: vec!["A saved query can be restored after restarting the app.".into()],
        in_scope: vec!["Persist and restore named search filters.".into()],
        out_of_scope: vec!["Sharing saved searches across users.".into()],
        constraints: vec!["Keep existing ad hoc search behavior compatible.".into()],
        ready_for_tasks: true,
    }
}
fn story() -> TaskStory {
    let response: crate::harness::responses::TaskStoryResponse =
        serde_json::from_str(include_str!("../../../tests/fixtures/task-story-1.json")).unwrap();
    let mut story = response.task_stories.unwrap().remove(0);
    story.success_criteria = vec![1];
    story
}

#[test]
fn small_task_needs_no_padding_and_incomplete_meaning_stays_invalid() {
    let small = TaskOutline {
        title: "Fix crash".into(),
        purpose: "Opening a saved filter crashes.".into(),
        scope_items: vec![1],
        success_criteria: vec![1],
        ..Default::default()
    };
    assert!(validate_outline(&brief(), &[small]).is_ok());

    let missing_references = TaskOutline {
        title: "Prepare the task batch".into(),
        purpose: "Separate the approved scope into implementable work.".into(),
        ..Default::default()
    };
    let feedback = validate_outline(&brief(), &[missing_references])
        .unwrap_err()
        .join(" ");
    assert!(feedback.contains("scope item index(es) [1]"));
    assert!(feedback.contains("success criterion index(es) [1]"));

    let mut concise = story();
    concise.title = "Show full blocker".into();
    concise.intent = "Long blocker reports hide the choices the user needs.".into();
    concise.goal = "Show the full report and let the user choose a response.".into();
    concise.context = "Task details currently clip long blocker reports.".into();
    concise.user_story = "As a blocked user, I need the whole issue and its reply control.".into();
    concise.purpose = "Restore the context needed to choose what happens next.".into();
    concise.affected_files = vec!["src/app/task_detail.rs: show the complete blocker".into()];
    concise.implementation_steps =
        vec!["Render the full report and its matching response control.".into()];
    concise.acceptance_criteria =
        vec!["A long blocker displays its full explanation and available choices.".into()];
    concise.test_plan =
        vec!["Render a long blocker and assert all text and choices remain visible.".into()];
    concise.verification_commands = vec!["cargo test --offline --lib task_detail".into()];
    concise.definition_of_done =
        vec!["The complete issue is visible and the user can submit a response.".into()];
    concise.technical_design.clear();
    concise.edge_cases.clear();
    concise.rollout_notes.clear();
    assert!(story_detail_errors(&concise, 1).is_empty());

    concise.intent.clear();
    concise.context = "Padding that does not explain the actual blocker. ".repeat(100);
    let errors = story_detail_errors(&concise, 1);
    assert!(
        errors
            .iter()
            .any(|error| error.contains("task-specific intent"))
    );
}

fn state(tag: &str) -> PlannerState {
    let root = std::env::temp_dir().join(format!("koolade_workflow_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success()
    );
    let mut s = PlannerState::load(&root).unwrap();
    s.bootstrap_missing().unwrap();
    s
}
fn envelope() -> TurnEnvelope {
    serde_json::from_value(serde_json::json!({"assistant_message":"The goal is to restore analysts' daily queries without repeating setup."})).unwrap()
}
fn mark_ready(state: &mut PlannerState) {
    let mut env = envelope();
    env.interview = Some(brief());
    let nt = validation::validate(&env, state, &state.effective_user()).unwrap();
    assert!(
        nt.assistant_message
            .contains("Generate task stories action")
    );
    apply::apply(state, &nt).unwrap();
}
fn generation(
    state: &PlannerState,
    stories: Vec<TaskStory>,
) -> Result<NormalizedTurn, Vec<String>> {
    let mut env = envelope();
    env.task_stories = Some(stories);
    validation::validate_for_turn(
        &env,
        state,
        &state.effective_user(),
        TurnPurpose::GenerateTasks,
    )
}

#[test]
fn readiness_persists_and_approved_stories_are_detailed_and_numbered() {
    let mut s = state("complete");
    mark_ready(&mut s);
    s = PlannerState::load(&s.repo_root).unwrap();
    assert!(s.workflow.ready(s.spec_text.as_deref()));
    assert!(
        !s.repo_root.join(".koolade-packet/planning/tasks").exists(),
        "readiness must not generate files"
    );
    let mut second = story();
    second.title = "Connect the saved search picker".into();
    second.dependencies = vec![1];
    let nt = generation(&s, vec![story(), second]).unwrap();
    let receipt = apply::apply(&mut s, &nt).unwrap();
    let dir = s
        .repo_root
        .join(".koolade-packet/planning/tasks/saved-searches");
    let task = std::fs::read_to_string(dir.join("001-persist-named-search-filters.md")).unwrap();
    assert!(task.contains(&story().intent));
    assert!(task.contains(&story().goal));
    assert!(!task.contains("## Product goal and intent"));
    assert!(!task.contains(&s.workflow.brief.as_ref().unwrap().goal));
    for section in [
        "Problem this ticket solves and why",
        "Ticket goal — what changes when done",
        "User story",
        "Implementation steps",
        "Acceptance criteria",
        "Test plan",
        "Edge cases",
        "Rollout",
        "Definition of done",
    ] {
        assert!(task.contains(section));
    }
    let second =
        std::fs::read_to_string(dir.join("002-connect-the-saved-search-picker.md")).unwrap();
    assert!(second.contains("[Task 001](001-persist-named-search-filters.md)"));
    assert_eq!(
        std::fs::read_to_string(dir.join("specification.md")).unwrap(),
        s.spec_text.clone().unwrap()
    );
    assert!(receipt.repo_relative_paths.contains(&WORKFLOW_FILE.into()));
    let restored = PlannerState::load(&s.repo_root).unwrap();
    assert!(!restored.workflow.ready(restored.spec_text.as_deref()));
    assert_eq!(
        crate::artifacts::task_docs::load_latest(&restored.repo_root, &restored.workflow).len(),
        2
    );
    assert!(
        generation(&restored, vec![story()]).is_err(),
        "approval is consumed after generation"
    );
    std::fs::remove_dir_all(s.repo_root).unwrap();
}

#[test]
fn no_approval_or_incomplete_intent_cannot_generate_tasks() {
    let mut s = state("consent");
    assert!(generation(&s, vec![story()]).is_err());
    let mut env = envelope();
    env.interview = Some(brief());
    env.interview.as_mut().unwrap().goal.clear();
    assert!(validation::validate(&env, &s, &s.effective_user()).is_err());
    mark_ready(&mut s);
    env = envelope();
    env.task_stories = Some(vec![story()]);
    assert!(validation::validate(&env, &s, &s.effective_user()).is_err());
    assert!(!s.repo_root.join(".koolade-packet/planning/tasks").exists());
    let nt = validation::validate(&envelope(), &s, &s.effective_user()).unwrap();
    apply::apply(&mut s, &nt).unwrap();
    assert!(
        !s.workflow.ready(s.spec_text.as_deref()),
        "further discussion invalidates old readiness"
    );
    std::fs::remove_dir_all(s.repo_root).unwrap();
}

#[test]
fn structured_generate_request_keeps_the_current_readiness_snapshot() {
    let mut state = state("typed-action");
    mark_ready(&mut state);
    let ready_before = state.workflow.clone();
    let mut env = envelope();
    env.requested_action = Some(crate::harness::RequestedAction {
        action: crate::harness::ApplicationAction::GenerateTasks,
        target_uid: None,
    });

    let normalized = validation::validate(&env, &state, &state.effective_user()).unwrap();
    assert_eq!(normalized.requested_action, env.requested_action);
    assert!(normalized.workflow.is_none());
    assert_eq!(state.workflow, ready_before);
    assert!(state.workflow.ready(state.planning_contract()));
    std::fs::remove_dir_all(state.repo_root).unwrap();
}

#[test]
fn structured_action_cannot_hide_a_planning_change() {
    let state = state("typed-action-mixed");
    let mut env = envelope();
    env.requested_action = Some(crate::harness::RequestedAction {
        action: crate::harness::ApplicationAction::ApproveChange,
        target_uid: None,
    });
    env.updated_specification = Some("# Unreviewed change".into());

    let problems = validation::validate(&env, &state, &state.effective_user()).unwrap_err();
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("cannot be combined"))
    );
    assert_eq!(state.items.len(), 0);
    std::fs::remove_dir_all(state.repo_root).unwrap();
}

#[path = "tests/feature_identity.rs"]
mod feature_identity;

#[path = "tests/generation_cases.rs"]
mod generation_cases;
