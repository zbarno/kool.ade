use super::{InterviewBrief, TaskOutline, descriptive_title, substantive};

const MAX_OUTLINE_PURPOSE_CHARS: usize = 500;

pub fn validate_outline(
    brief: &InterviewBrief,
    outline: &[TaskOutline],
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let mut titles = std::collections::HashSet::new();
    let mut scope = std::collections::HashSet::new();
    let mut criteria = std::collections::HashSet::new();
    if outline.is_empty() || outline.len() > 200 {
        errors.push("The outline must contain between 1 and 200 tasks.".into());
    }
    for (i, task) in outline.iter().enumerate() {
        let n = i + 1;
        if !descriptive_title(&task.title)
            || !substantive(&task.purpose)
            || !titles.insert(task.title.trim().to_lowercase())
        {
            errors.push(format!(
                "Outline task {n}: supply a unique title and concrete purpose."
            ));
        }
        if task.purpose.chars().count() > MAX_OUTLINE_PURPOSE_CHARS {
            errors.push(format!(
                "Outline task {n}: keep its purpose to {MAX_OUTLINE_PURPOSE_CHARS} characters or fewer; state only this task's reason."
            ));
        }
        if task.dependencies.iter().any(|d| *d == 0 || *d >= n) {
            errors.push(format!(
                "Outline task {n}: dependencies must refer to earlier tasks."
            ));
        }
        if task
            .parent_task_index
            .is_some_and(|parent| parent == 0 || parent >= n)
        {
            errors.push(format!(
                "Outline task {n}: parent_task_index must refer to an earlier task."
            ));
        }
        if task.scope_items.is_empty() {
            errors.push(format!(
                "Outline task {n}: at least one scope reference is required."
            ));
        }
        for r in &task.scope_items {
            if *r == 0 || *r > brief.in_scope.len() {
                errors.push(format!(
                    "Outline task {n}: scope index {r} is invalid; use an index from 1 through {}.",
                    brief.in_scope.len()
                ));
            } else {
                scope.insert(*r);
            }
        }
        for r in &task.success_criteria {
            if *r == 0 || *r > brief.success_criteria.len() {
                errors.push(format!(
                    "Outline task {n}: success-criterion index {r} is invalid; use an index from 1 through {}.",
                    brief.success_criteria.len()
                ));
            } else {
                criteria.insert(*r);
            }
        }
    }
    for (i, task) in outline.iter().enumerate() {
        if task
            .parent_task_index
            .and_then(|parent| parent.checked_sub(1))
            .and_then(|parent| outline.get(parent))
            .is_some_and(|parent| parent.parent_task_index.is_some())
        {
            errors.push(format!(
                "Outline task {}: a subtask cannot itself own subtasks.",
                i + 1
            ));
        }
    }
    let missing_scope = (1..=brief.in_scope.len())
        .filter(|index| !scope.contains(index))
        .collect::<Vec<_>>();
    if !missing_scope.is_empty() {
        errors.push(format!(
            "The outline is missing scope item index(es) {missing_scope:?}; assign each to a task."
        ));
    }
    let missing_criteria = (1..=brief.success_criteria.len())
        .filter(|index| !criteria.contains(index))
        .collect::<Vec<_>>();
    if !missing_criteria.is_empty() {
        errors.push(format!(
            "The outline is missing success criterion index(es) {missing_criteria:?}; assign each to a task."
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_outline_purposes_longer_than_story_budget() {
        let brief = InterviewBrief {
            in_scope: vec!["Implement the approved change".into()],
            success_criteria: vec!["The change is verified".into()],
            ..Default::default()
        };
        let task = TaskOutline {
            title: "Implement approved change".into(),
            purpose: "The specific reason for this task. ".repeat(20),
            scope_items: vec![1],
            success_criteria: vec![1],
            ..Default::default()
        };
        let errors = validate_outline(&brief, &[task]).unwrap_err().join(" ");
        assert!(errors.contains("500 characters or fewer"));
    }

    #[test]
    fn invalid_outline_references_report_the_allowed_range() {
        let brief = InterviewBrief {
            in_scope: vec!["One scope item".into()],
            success_criteria: vec!["One criterion".into()],
            ..Default::default()
        };
        let task = TaskOutline {
            title: "Implement the change".into(),
            purpose: "Deliver the approved behavior.".into(),
            scope_items: vec![2],
            success_criteria: vec![2],
            ..Default::default()
        };
        let errors = validate_outline(&brief, &[task]).unwrap_err().join(" ");
        assert!(errors.contains("scope index 2 is invalid; use an index from 1 through 1"));
        assert!(
            errors.contains("success-criterion index 2 is invalid; use an index from 1 through 1")
        );
    }

    #[test]
    fn parent_with_independent_children_is_a_valid_parallel_subtask_dag() {
        let brief = InterviewBrief {
            in_scope: vec!["Deliver the approved behavior".into()],
            success_criteria: vec!["The behavior is verified".into()],
            ..Default::default()
        };
        let outline = vec![
            TaskOutline {
                title: "Coordinate the implementation".into(),
                purpose: "Own the integrated result and combine the child changes.".into(),
                scope_items: vec![1],
                success_criteria: vec![1],
                ..Default::default()
            },
            TaskOutline {
                title: "Implement the first independent slice".into(),
                purpose: "Complete one bounded part of the approved behavior.".into(),
                scope_items: vec![1],
                success_criteria: vec![1],
                parent_task_index: Some(1),
                ..Default::default()
            },
            TaskOutline {
                title: "Implement the second independent slice".into(),
                purpose: "Complete a separate bounded part of the approved behavior.".into(),
                scope_items: vec![1],
                success_criteria: vec![1],
                parent_task_index: Some(1),
                ..Default::default()
            },
        ];
        assert!(validate_outline(&brief, &outline).is_ok());
    }

    #[test]
    fn rejects_nested_subtask_ownership() {
        let brief = InterviewBrief {
            in_scope: vec!["Deliver the approved behavior".into()],
            success_criteria: vec!["The behavior is verified".into()],
            ..Default::default()
        };
        let outline = vec![
            TaskOutline {
                title: "Coordinate the implementation".into(),
                purpose: "Own the integrated result and combine child changes.".into(),
                scope_items: vec![1],
                success_criteria: vec![1],
                ..Default::default()
            },
            TaskOutline {
                title: "Own an implementation slice".into(),
                purpose: "Coordinate the implementation of one bounded slice.".into(),
                scope_items: vec![1],
                success_criteria: vec![1],
                parent_task_index: Some(1),
                ..Default::default()
            },
            TaskOutline {
                title: "Implement a nested detail".into(),
                purpose: "Complete one detail within the bounded slice.".into(),
                scope_items: vec![1],
                success_criteria: vec![1],
                parent_task_index: Some(2),
                ..Default::default()
            },
        ];
        assert!(
            validate_outline(&brief, &outline)
                .unwrap_err()
                .iter()
                .any(|error| error.contains("cannot itself own subtasks"))
        );
    }
}
