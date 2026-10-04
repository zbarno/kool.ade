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
        let path = state.repo_root.join(&batch.directory).join("contract.json");
        let Ok(text) = std::fs::read_to_string(path) else {
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
    let directory = state.repo_root.join(&batch.directory);
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
        let relative = path
            .strip_prefix(&state.repo_root)?
            .to_string_lossy()
            .into_owned();
        let Some(record) = crate::core::implementation::load(&state.repo_root, &relative) else {
            return Ok(None);
        };
        if record.status != ImplementationStatus::Completed || record.merged_commit.is_none() {
            return Ok(None);
        }
        anyhow::ensure!(
            crate::artifacts::task_docs::visible_content(&std::fs::read_to_string(&path)?)
                == record.ticket_text,
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
