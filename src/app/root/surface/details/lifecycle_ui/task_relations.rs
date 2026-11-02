use crate::app::session::Project;

pub(super) fn relation_for_planning_uid(
    project: &Project,
    planning_uid: Option<&str>,
) -> Option<crate::ui::task_detail::TaskRelationView> {
    let mut matches = project.task_documents.iter().filter(|document| {
        document
            .metadata
            .as_ref()
            .is_some_and(|metadata| Some(metadata.uid.as_str()) == planning_uid)
    });
    let document = matches.next()?;
    if matches.next().is_some() {
        return Some(unavailable_relation(planning_uid?));
    }
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&project.state.planning_store);
    let lifecycle_task_result = lifecycle.task_for_planning_item(planning_uid?);
    let lifecycle_task = lifecycle_task_result.as_ref().ok().and_then(Option::as_ref);
    let status = if lifecycle_task_result.is_err() {
        "Lifecycle status unavailable".to_owned()
    } else {
        lifecycle_task.map_or_else(
            || {
                project
                    .implementation_states
                    .get(&document.path)
                    .map(|state| state.status.label().to_owned())
                    .unwrap_or_else(|| "Status not recorded".to_owned())
            },
            |task| {
                let phase = crate::ui::task_detail::task_phase_label(task.phase);
                let pr_status = task.pr_work_item_uid.as_deref().map(|uid| {
                    lifecycle.load_pr_preparation(uid).map_or(
                        "PR status unavailable",
                        |pr| match pr.status {
                            crate::core::lifecycle::PullRequestPreparationStatus::AwaitingAuthorization => {
                                "PR awaiting authorization"
                            }
                            crate::core::lifecycle::PullRequestPreparationStatus::Publishing => {
                                "PR publishing"
                            }
                            crate::core::lifecycle::PullRequestPreparationStatus::NeedsAttention => {
                                "PR needs attention"
                            }
                            crate::core::lifecycle::PullRequestPreparationStatus::Opened => "PR open",
                            crate::core::lifecycle::PullRequestPreparationStatus::ClosedUnmerged => {
                                "PR closed unmerged"
                            }
                            crate::core::lifecycle::PullRequestPreparationStatus::Merged => "PR merged",
                        },
                    )
                });
                pr_status.map_or_else(|| phase.to_owned(), |pr| format!("{phase} · {pr}"))
            },
        )
    };
    let lifecycle_task_uid = lifecycle_task.map(|task| task.task_uid.clone());
    Some(crate::ui::task_detail::TaskRelationView {
        planning_item_uid: planning_uid?.to_owned(),
        lifecycle_task_uid,
        title: document.title.clone(),
        status,
        acceptance_criteria: acceptance_criteria(&document.text),
    })
}

pub(super) fn unavailable_relation(uid: &str) -> crate::ui::task_detail::TaskRelationView {
    crate::ui::task_detail::TaskRelationView {
        planning_item_uid: uid.to_owned(),
        lifecycle_task_uid: None,
        title: "Related task unavailable".into(),
        status: "Relationship data incomplete".into(),
        acceptance_criteria: Vec::new(),
    }
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
    use super::{acceptance_criteria, relation_for_planning_uid, unavailable_relation};

    #[test]
    fn duplicate_task_uids_project_as_unavailable_instead_of_navigating_to_first_match() {
        let temp = tempfile::tempdir().unwrap();
        let mut project = crate::app::session::test_support::project_from(temp.path());
        let mut identity = crate::domain::ArtifactIdentity::new("TASK-DUP", "First duplicate");
        identity.parent_uid = Some(uuid::Uuid::new_v4().to_string());
        let metadata =
            crate::artifacts::task_docs::TaskMetadata::new(&identity, "root", Vec::new()).unwrap();
        for (path, title) in [
            ("planning/tasks/first.md", "First duplicate"),
            ("planning/tasks/second.md", "Second duplicate"),
        ] {
            project
                .task_documents
                .push(crate::artifacts::task_docs::TaskDocument {
                    path: path.into(),
                    title: title.into(),
                    text: String::new(),
                    identity: Some(identity.clone()),
                    metadata: Some(metadata.clone()),
                    task_state: None,
                    metadata_error: None,
                });
        }

        let relation = relation_for_planning_uid(&project, Some(&identity.uid)).unwrap();

        assert_eq!(relation.planning_item_uid, identity.uid);
        assert_eq!(relation.title, "Related task unavailable");
        assert_eq!(relation.status, "Relationship data incomplete");
        assert_eq!(relation.lifecycle_task_uid, None);
    }

    #[test]
    fn acceptance_criteria_are_read_only_and_bounded_to_their_markdown_section() {
        let markdown = "## Acceptance criteria\n\n1. First outcome is visible.\n- Second outcome persists.\n\n## Test plan\n\n- Do not show this line.";
        assert_eq!(
            acceptance_criteria(markdown),
            vec!["First outcome is visible.", "Second outcome persists."]
        );
    }

    #[test]
    fn unavailable_relationship_preserves_its_reference_without_a_navigable_task() {
        let relation = unavailable_relation("missing-task-uid");
        assert_eq!(relation.planning_item_uid, "missing-task-uid");
        assert_eq!(relation.title, "Related task unavailable");
        assert_eq!(relation.status, "Relationship data incomplete");
        assert_eq!(relation.lifecycle_task_uid, None);
    }
}
