use crate::core::workflow::{InterviewBrief, TaskOutline, TaskStory};

pub(crate) fn flatten(
    graph: crate::harness::prompt_catalog::TaskGraphV1,
    snapshot: &crate::core::lifecycle::PlanSnapshotV1,
    brief: &InterviewBrief,
) -> anyhow::Result<(Vec<TaskOutline>, Vec<TaskStory>)> {
    let mut roots: Vec<&crate::harness::prompt_catalog::TaskDraftV1> = Vec::new();
    let mut remaining = graph.tasks.iter().collect::<Vec<_>>();
    while !remaining.is_empty() {
        let completed = roots
            .iter()
            .map(|task| task.uid.as_str())
            .collect::<std::collections::HashSet<_>>();
        let Some(index) = remaining.iter().position(|task| {
            task.dependencies
                .iter()
                .all(|dependency| completed.contains(dependency.as_str()))
        }) else {
            anyhow::bail!("Validated task graph has no topological order");
        };
        roots.push(remaining.remove(index));
    }
    let mut all = Vec::<(
        String,
        Option<String>,
        Vec<String>,
        String,
        String,
        String,
        Vec<String>,
        Vec<String>,
        Vec<String>,
        Vec<String>,
        String,
    )>::new();
    for root in roots {
        all.push((
            root.uid.clone(),
            None,
            root.dependencies.clone(),
            root.title.clone(),
            root.goal.clone(),
            root.intent.clone(),
            root.non_goals.clone(),
            root.relevant_files.clone(),
            root.acceptance_criteria.clone(),
            root.tests.clone(),
            root.repository_id.clone(),
        ));
        let mut children = root.subtasks.iter().collect::<Vec<_>>();
        let mut ordered = Vec::new();
        while !children.is_empty() {
            let completed = ordered
                .iter()
                .map(|child: &&crate::harness::prompt_catalog::SubtaskDraftV1| child.uid.as_str())
                .collect::<std::collections::HashSet<_>>();
            let Some(index) = children.iter().position(|child| {
                child
                    .depends_on
                    .iter()
                    .all(|dependency| completed.contains(dependency.as_str()))
            }) else {
                anyhow::bail!("Validated subtask graph has no topological order");
            };
            ordered.push(children.remove(index));
        }
        let mut previous_child: Option<&str> = None;
        let mut previous_was_parallelizable = true;
        for child in ordered {
            let mut dependencies = child.depends_on.clone();
            if (!child.parallelizable || !previous_was_parallelizable)
                && let Some(previous) = previous_child
                && !dependencies.iter().any(|dependency| dependency == previous)
            {
                // The existing scheduler uses dependency edges as its concurrency
                // contract, so translate an explicit serial subtask into an edge.
                dependencies.push(previous.to_owned());
            }
            all.push((
                child.uid.clone(),
                Some(root.uid.clone()),
                dependencies,
                child.title.clone(),
                child.goal.clone(),
                child.goal.clone(),
                Vec::new(),
                if child.affected_paths.is_empty() {
                    root.relevant_files.clone()
                } else {
                    child.affected_paths.clone()
                },
                child.acceptance_criteria.clone(),
                child.tests.clone(),
                root.repository_id.clone(),
            ));
            previous_child = Some(&child.uid);
            previous_was_parallelizable = child.parallelizable;
        }
    }
    anyhow::ensure!(
        !brief.in_scope.is_empty(),
        "Approved brief has no scope items"
    );
    let uid_to_index = all
        .iter()
        .enumerate()
        .map(|(index, item)| (item.0.as_str(), index + 1))
        .collect::<std::collections::HashMap<_, _>>();
    let mut outlines = Vec::new();
    let mut stories = Vec::new();
    for (
        index,
        (
            uid,
            parent_uid,
            dependencies,
            title,
            goal,
            intent,
            non_goals,
            files,
            criteria,
            tests,
            repository,
        ),
    ) in all.iter().enumerate()
    {
        let dep_indexes = dependencies
            .iter()
            .map(|dependency| {
                uid_to_index
                    .get(dependency.as_str())
                    .copied()
                    .ok_or_else(|| {
                        anyhow::anyhow!("Validated graph dependency was lost while flattening")
                    })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let parent_index = parent_uid
            .as_ref()
            .and_then(|parent| uid_to_index.get(parent.as_str()).copied());
        let scope_items = if index == 0 {
            (1..=brief.in_scope.len()).collect()
        } else {
            vec![1]
        };
        let success_criteria = if index == 0 {
            (1..=brief.success_criteria.len()).collect()
        } else {
            Vec::new()
        };
        let requirement_ids = graph
            .tasks
            .iter()
            .find(|task| task.uid == *uid)
            .map(|task| task.requirements.clone())
            .unwrap_or_default();
        let source_sha = graph
            .tasks
            .iter()
            .find(|task| task.uid == *uid || parent_uid.as_deref() == Some(task.uid.as_str()))
            .and_then(|task| task.source_sha.clone());
        let concurrency_risks = graph
            .tasks
            .iter()
            .find(|task| task.uid == *uid || parent_uid.as_deref() == Some(task.uid.as_str()))
            .map(|task| task.estimated_concurrency_risks.clone())
            .unwrap_or_default();
        let parallelizable = graph
            .tasks
            .iter()
            .flat_map(|task| task.subtasks.iter())
            .find(|child| child.uid == *uid)
            .map(|child| child.parallelizable);
        let non_goals_context = if non_goals.is_empty() {
            String::new()
        } else {
            format!("\nTask non-goals:\n- {}", non_goals.join("\n- "))
        };
        let story = TaskStory {
            graph_uid: Some(uid.clone()),
            source_sha,
            concurrency_risks,
            parallelizable,
            approved_requirement_ids: requirement_ids,
            title: title.clone(),
            target_repository: repository.clone(),
            intent: intent.clone(),
            goal: goal.clone(),
            context: format!(
                "Approved plan intent: {}{}",
                snapshot.intent, non_goals_context
            ),
            technical_design: files.clone(),
            verification_commands: tests.clone(),
            user_story: format!("Implement the approved task outcome: {goal}"),
            purpose: goal.clone(),
            scope_items: scope_items.clone(),
            success_criteria: success_criteria.clone(),
            dependencies: dep_indexes,
            parent_task_index: parent_index,
            affected_files: files.clone(),
            implementation_steps: criteria.clone(),
            acceptance_criteria: criteria.clone(),
            test_plan: tests.clone(),
            edge_cases: Vec::new(),
            rollout_notes: String::new(),
            definition_of_done: criteria.clone(),
        };
        outlines.push(TaskOutline {
            title: title.clone(),
            purpose: goal.clone(),
            target_repository: repository.clone(),
            scope_items,
            success_criteria,
            dependencies: story.dependencies.clone(),
            parent_task_index: parent_index,
        });
        stories.push(story);
    }
    crate::core::workflow::validate_outline(brief, &outlines)
        .map_err(|errors| anyhow::anyhow!(errors.join("; ")))?;
    Ok((outlines, stories))
}
