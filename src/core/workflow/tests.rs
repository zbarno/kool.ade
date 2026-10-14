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

#[path = "tests/brief.rs"]
mod brief;

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
    for args in [
        ["config", "user.name", "Kool.ad/e Test"].as_slice(),
        ["config", "user.email", "koolade@test.local"].as_slice(),
    ] {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        );
    }
    let mut s = PlannerState::load(&root).unwrap();
    s.bootstrap_missing().unwrap();
    s
}

fn install_approved_feature(state: &mut PlannerState) {
    let root = state.repo_root.clone();
    let id = "CHG-004";
    let dir = root.join(".koolade-packet/planning/changes/CHG-004-saved-searches");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("specification.md");
    let markdown = "# CHG-004: Saved searches\n\n**Status:** Ready\n\n## Intent\n\nLet analysts resume a useful query.\n\n## Current Behavior\n\nAnalysts reconstruct query configuration each morning.\n\n## Desired Behavior\n\nAnalysts can restore a saved query.\n\n## Scope\n\nPersist and restore named search filters.\n\n## Affected Product Areas\n\n`product:current-capabilities`\n\n## Requirements\n\n- SEARCH-R1 (MUST). Restore a named query after restart.\n\n## Decisions and Assumptions\n\n- **A1 (fixture):** Saved searches remain local.\n\n## Acceptance Criteria\n\n1. A named query restores after restart.\n";
    let markdown =
        crate::artifacts::product_docs::identity::preserve_feature_identity(&path, id, markdown)
            .unwrap();
    let identity = crate::domain::ArtifactIdentity::from_markdown(&markdown)
        .unwrap()
        .unwrap();
    let markdown = crate::domain::ChangeMetadata::write_markdown(
        &markdown,
        &identity,
        crate::domain::ChangeStatus::Ready,
    )
    .unwrap();
    let relative = path
        .strip_prefix(state.planning_store.root.as_path())
        .unwrap()
        .to_string_lossy()
        .into_owned();
    state
        .planning_store
        .transaction(
            &[(relative, markdown.as_bytes().to_vec())],
            Some(&state.baseline_planning_revision),
        )
        .unwrap();
    state.workflow.brief = Some(brief());
    crate::artifacts::task_docs::save_workflow(&root, &state.workflow).unwrap();
    state.baseline_planning_revision = state.planning_store.revision().unwrap();
    crate::core::gitops::commit(
        &root,
        "fixture: create approved saved-search feature",
        &[".koolade-packet/planning".into()],
    )
    .unwrap();
    state.active_feature = Some((id.into(), markdown.clone()));
    state.active_features = vec![(id.into(), markdown.clone())];
    crate::core::lifecycle::test_support::approve_and_review_feature(
        &state.planning_store,
        &mut state.workflow,
        id,
        &markdown,
    )
    .unwrap();
    state.baseline_planning_revision = state.planning_store.revision().unwrap();
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
    install_approved_feature(&mut s);
    mark_ready(&mut s);
    s = PlannerState::load(&s.repo_root).unwrap();
    assert!(s.workflow.ready(s.planning_contract()));
    assert!(
        !s.repo_root.join(".koolade-packet/planning/tasks").exists(),
        "readiness must not generate files"
    );
    let mut second = story();
    second.title = "Connect the saved search picker".into();
    second.dependencies = vec![1];
    let nt = generation(&s, vec![story(), second]).unwrap();
    let receipt = apply::apply(&mut s, &nt).unwrap();
    let task_path = receipt
        .repo_relative_paths
        .iter()
        .find(|path| path.ends_with("TASK-persist-named-search-filters.md"))
        .expect("generated task path is recorded");
    let task = std::fs::read_to_string(s.repo_root.join(task_path)).unwrap();
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
    let second_task_path = receipt
        .repo_relative_paths
        .iter()
        .find(|path| path.ends_with("TASK-connect-the-saved-search-picker.md"))
        .expect("generated dependent task path is recorded");
    let second = std::fs::read_to_string(s.repo_root.join(second_task_path)).unwrap();
    assert!(second.contains("TASK-persist-named-search-filters.md"));
    let specification_path = receipt
        .repo_relative_paths
        .iter()
        .find(|path| path.ends_with("/specification.md"))
        .expect("generated batch specification path is recorded");
    let approved_feature_path =
        crate::artifacts::product_docs::document_path(&s.planning_store, "feature:CHG-004")
            .unwrap();
    assert_eq!(
        std::fs::read_to_string(s.repo_root.join(specification_path)).unwrap(),
        std::fs::read_to_string(approved_feature_path).unwrap()
    );
    assert!(
        receipt
            .repo_relative_paths
            .iter()
            .any(|path| path.starts_with(".koolade-packet/state/workflow/"))
    );
    let restored = PlannerState::load(&s.repo_root).unwrap();
    assert!(!restored.workflow.ready(restored.planning_contract()));
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
        !s.workflow.ready(s.planning_contract()),
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

    let normalized = validation::validate_for_turn_with_user_message(
        &env,
        &state,
        &state.effective_user(),
        crate::core::workflow::TurnPurpose::Interview,
        &[],
        "Generate tasks for the current feature",
    )
    .unwrap();
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
