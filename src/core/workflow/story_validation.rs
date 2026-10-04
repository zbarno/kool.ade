use super::{InterviewBrief, TaskStory, substantive};
use std::collections::HashSet;

/// Reject empty, multiline, or unwieldy titles without imposing a word-count target.
pub fn descriptive_title(title: &str) -> bool {
    substantive(title) && title.chars().count() <= 140 && !title.contains(['\n', '\r'])
}

/// Check for implementable meaning while letting the task's scope determine its length.
pub fn story_detail_errors(story: &TaskStory, number: usize) -> Vec<String> {
    let mut errors = Vec::new();
    if !descriptive_title(&story.title) {
        errors.push(format!(
            "Task {number}: provide a concise, specific title (maximum 140 characters)."
        ));
    }

    for (name, value) in [
        ("intent", &story.intent),
        ("goal", &story.goal),
        ("context", &story.context),
        ("user story", &story.user_story),
        ("purpose", &story.purpose),
    ] {
        if !substantive(value) {
            errors.push(format!(
                "Task {number}: add the task-specific {name}; keep it concise and explain only what this task needs."
            ));
        }
    }

    for (name, values) in [
        ("affected files or components", &story.affected_files),
        ("implementation steps", &story.implementation_steps),
        ("acceptance criteria", &story.acceptance_criteria),
        ("test plan", &story.test_plan),
        ("verification expectations", &story.verification_commands),
        ("definition of done", &story.definition_of_done),
    ] {
        validate_entries(&mut errors, number, name, values, true);
    }

    for (name, values) in [
        ("technical design", &story.technical_design),
        ("edge cases", &story.edge_cases),
    ] {
        validate_entries(&mut errors, number, name, values, false);
    }

    if !story.rollout_notes.trim().is_empty() && !substantive(&story.rollout_notes) {
        errors.push(format!(
            "Task {number}: replace the rollout placeholder with relevant details or leave it empty."
        ));
    }
    errors
}

fn validate_entries(
    errors: &mut Vec<String>,
    number: usize,
    name: &str,
    values: &[String],
    required: bool,
) {
    if required && values.is_empty() {
        errors.push(format!(
            "Task {number}: include the {name} needed for this task."
        ));
        return;
    }

    let mut seen = HashSet::new();
    for value in values {
        let normalized = value.trim().to_lowercase();
        if !substantive(value) {
            errors.push(format!(
                "Task {number}: remove the empty or placeholder entry from {name}."
            ));
            break;
        }
        if !seen.insert(normalized) {
            errors.push(format!(
                "Task {number}: remove repeated entries from {name}."
            ));
            break;
        }
    }
}

pub(super) fn validate_stories(
    brief: &InterviewBrief,
    stories: &[TaskStory],
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if stories.is_empty() || stories.len() > 200 {
        errors.push("Return between 1 and 200 complete task stories.".into());
    }
    let mut titles = HashSet::new();
    let mut scope = HashSet::new();
    let mut criteria = HashSet::new();
    for (index, story) in stories.iter().enumerate() {
        let number = index + 1;
        if !titles.insert(story.title.trim().to_lowercase()) {
            errors.push(format!("Task {number}: duplicate title."));
        }
        errors.extend(story_detail_errors(story, number));
        if story
            .dependencies
            .iter()
            .any(|dependency| *dependency == 0 || *dependency >= number)
        {
            errors.push(format!(
                "Task {number}: dependencies must reference earlier tasks, preventing missing references and cycles."
            ));
        }
        if story.scope_items.is_empty() {
            errors.push(format!(
                "Task {number}: reference at least one approved scope item."
            ));
        }
        for reference in &story.scope_items {
            if *reference == 0 || *reference > brief.in_scope.len() {
                errors.push(format!(
                    "Task {number}: invalid scope reference {reference}."
                ));
            } else {
                scope.insert(*reference);
            }
        }
        for reference in &story.success_criteria {
            if *reference == 0 || *reference > brief.success_criteria.len() {
                errors.push(format!(
                    "Task {number}: invalid success criterion {reference}."
                ));
            } else {
                criteria.insert(*reference);
            }
        }
    }
    if scope.len() != brief.in_scope.len() || criteria.len() != brief.success_criteria.len() {
        errors.push(
            "Task stories must cover every approved scope item and success criterion.".into(),
        );
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
