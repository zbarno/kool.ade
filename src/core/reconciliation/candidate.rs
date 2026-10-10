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
        if record.status != ImplementationStatus::Completed || record.merged_commit.is_none() {
            return Ok(None);
        }
        anyhow::ensure!(
            crate::artifacts::task_docs::visible_content(&String::from_utf8(
                state.planning_store.read_planning_path(&path)?,
            )?) == record.ticket_text,
            "Task story changed after implementation: {relative}"
        );
        tasks.push(record);
    }
    Ok(Some(Candidate {
        feature_id: feature_id.clone(),
        batch_directory: batch.directory.clone(),
        contract,
        tasks,
    }))
}
