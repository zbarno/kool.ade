use crate::app::session::Project;

pub(super) fn relation_for_planning_uid(
    project: &Project,
    planning_uid: Option<&str>,
) -> Option<crate::ui::task_detail::TaskRelationView> {
    let document = project.task_documents.iter().find(|document| {
        document
            .metadata
            .as_ref()
            .is_some_and(|metadata| Some(metadata.uid.as_str()) == planning_uid)
    })?;
    let status = project
        .implementation_states
        .get(&document.path)
        .map(|state| state.status.label().to_owned())
        .unwrap_or_else(|| "Not started".to_owned());
    let lifecycle_task_uid =
        crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store)
            .task_for_planning_item(planning_uid?)
            .ok()
            .flatten()
            .map(|task| task.task_uid);
    Some(crate::ui::task_detail::TaskRelationView {
        planning_item_uid: planning_uid?.to_owned(),
        lifecycle_task_uid,
        title: document.title.clone(),
        status,
        acceptance_criteria: acceptance_criteria(&document.text),
    })
}

fn acceptance_criteria(markdown: &str) -> Vec<String> {
    let mut in_section = false;
    markdown
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.eq_ignore_ascii_case("## acceptance criteria") {
                in_section = true;
                return None;
            }
            if trimmed.starts_with("## ") {
                in_section = false;
                return None;
            }
            if !in_section {
                return None;
            }
            let criterion = trimmed.strip_prefix("- ").or_else(|| {
                let (prefix, rest) = trimmed.split_once(". ")?;
                (!prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()))
                    .then_some(rest)
            })?;
            (!criterion.is_empty()).then(|| criterion.to_owned())
        })
        .take(64)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::acceptance_criteria;

    #[test]
    fn acceptance_criteria_are_read_only_and_bounded_to_their_markdown_section() {
        let markdown = "## Acceptance criteria\n\n1. First outcome is visible.\n- Second outcome persists.\n\n## Test plan\n\n- Do not show this line.";
        assert_eq!(
            acceptance_criteria(markdown),
            vec!["First outcome is visible.", "Second outcome persists."]
        );
    }
}
