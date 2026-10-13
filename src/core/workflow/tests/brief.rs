use super::*;

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
