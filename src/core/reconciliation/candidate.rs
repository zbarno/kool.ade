use super::*;

#[derive(Debug, Clone)]
pub struct Candidate {
    pub feature_id: String,
    pub batch_directory: String,
    pub contract: crate::core::contract_snapshot::BatchContract,
    pub tasks: Vec<crate::core::implementation::Implementation>,
}

fn numbered_story(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(crate::artifacts::task_docs::is_task_story_filename)
}

pub fn candidate(state: &PlannerState) -> anyhow::Result<Option<Candidate>> {
    let Some((feature_id, _)) = &state.active_feature else {
        return Ok(None);
    };
    if state.items.iter().any(|item| {
        item.feature_id.as_deref() == Some(feature_id)
            && matches!(item.authority, Authority::Review | Authority::Human)
            && item.question.to_ascii_lowercase().contains("reconcil")
    }) {
        return Ok(None);
    }
    let mut selected = None;
    for batch in state.workflow.task_batches.iter().rev() {
        let Some(path) = state
            .planning_store
            .layout()
            .canonical_path(&format!("{}/contract.json", batch.directory))
        else {
            continue;
        };
        let Ok(bytes) = state.planning_store.read_planning_path(&path) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let Ok(contract) =
            serde_json::from_str::<crate::core::contract_snapshot::BatchContract>(&text)
        else {
            continue;
        };
        if contract.feature_id == *feature_id {
            selected = Some((batch, contract));
            break;
        }
    }
    let Some((batch, contract)) = selected else {
        return Ok(None);
    };
    let directory = state
        .planning_store
        .layout()
        .canonical_path(&batch.directory)
        .ok_or_else(|| anyhow::anyhow!("Task batch directory is outside planning root"))?;
    let mut stories = std::fs::read_dir(&directory)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| numbered_story(path))
        .collect::<Vec<_>>();
    stories.sort();
    anyhow::ensure!(
        stories.len() == batch.count && !stories.is_empty(),
        "Task batch story count changed"
    );
    let mut tasks = Vec::with_capacity(stories.len());
    for path in stories {
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("Task story filename is not UTF-8"))?;
        let relative = format!("{}/{filename}", batch.directory);
        let Some(record) = crate::core::implementation::load_with_store(
            &state.planning_store,
            &state.repo_root,
            &relative,
        ) else {
            return Ok(None);
        };
        anyhow::ensure!(
            crate::artifacts::task_docs::visible_content(&String::from_utf8(
                state.planning_store.read_planning_path(&path)?,
            )?) == record.ticket_text,
            "Task story changed after implementation: {relative}"
        );
        tasks.push(record);
    }
    if !all_required_repositories_merged(&tasks) {
        return Ok(None);
    }
    Ok(Some(Candidate {
        feature_id: feature_id.clone(),
        batch_directory: batch.directory.clone(),
        contract,
        tasks,
    }))
}

fn all_required_repositories_merged(tasks: &[crate::core::implementation::Implementation]) -> bool {
    !tasks.is_empty()
        && tasks.iter().all(|task| {
            task.status == ImplementationStatus::Completed && task.merged_commit.is_some()
        })
}

#[cfg(test)]
mod tests {
    use super::all_required_repositories_merged;
    use crate::core::implementation::Implementation;

    fn task(repository_id: &str, status: &str, merged_commit: Option<&str>) -> Implementation {
        serde_json::from_value(serde_json::json!({
            "ticket": format!("{repository_id}.md"),
            "ticket_text": "",
            "branch": "koolade/task",
            "base": "main",
            "base_commit": "0000000000000000000000000000000000000000",
            "task_repository": "/tmp/koolade-task",
            "status": status,
            "detail": "",
            "pr_url": null,
            "verified_head": null,
            "repository_id": repository_id,
            "merged_commit": merged_commit
        }))
        .unwrap()
    }

    #[test]
    fn a_multi_repository_feature_waits_for_every_story_merge() {
        let first = task(
            "api",
            "completed",
            Some("1111111111111111111111111111111111111111"),
        );
        let second_open = task("web", "awaiting_review", None);
        assert!(!all_required_repositories_merged(&[
            first.clone(),
            second_open
        ]));

        let second = task(
            "web",
            "completed",
            Some("2222222222222222222222222222222222222222"),
        );
        assert!(all_required_repositories_merged(&[first, second]));
    }
}
