use crate::artifacts::planning_store::PlanningStore;
use crate::core::workflow::Workflow;

type TaskStateBackfillPlan = (
    String,
    Vec<(String, Vec<u8>)>,
    Vec<crate::artifacts::planning_store::RecordRevisionCheck>,
);

pub(crate) fn backfill_missing(
    store: &PlanningStore,
    workflow: &Workflow,
) -> anyhow::Result<Vec<String>> {
    for attempt in 0..3 {
        let (expected_revision, changes, checks) = plan_missing(store, workflow)?;
        if changes.is_empty() {
            return Ok(Vec::new());
        }
        match store.transaction_with_revision_and_record_revisions(
            &changes,
            Some(&expected_revision),
            &checks,
        ) {
            Ok((paths, _)) => {
                return Ok(paths.iter().map(|path| store.git_path(path)).collect());
            }
            Err(
                crate::artifacts::planning_store::StoreError::StaleRevision { .. }
                | crate::artifacts::planning_store::StoreError::StaleRecordRevision { .. },
            ) if attempt < 2 => {}
            Err(error) => return Err(error.into()),
        }
    }
    anyhow::bail!("Task records kept changing during migration; retry connection")
}

pub(crate) fn plan_missing(
    store: &PlanningStore,
    workflow: &Workflow,
) -> anyhow::Result<TaskStateBackfillPlan> {
    let (expected_revision, documents) = store.with_consistent_read(|| {
        let revision = store.revision()?;
        let documents = super::super::board::load_board_unlocked(store, workflow);
        Ok::<_, anyhow::Error>((revision, documents))
    })?;
    let registered_batches = workflow
        .task_batches
        .iter()
        .filter_map(|batch| super::super::identity::resolve_batch_directory(store, batch))
        .collect::<std::collections::BTreeSet<_>>();
    let mut changes = Vec::new();
    let mut checks = Vec::new();
    let mut by_batch_and_name = std::collections::BTreeMap::new();
    for document in &documents {
        let Some((directory, _)) = document.path.rsplit_once('/') else {
            continue;
        };
        if !registered_batches.contains(directory) {
            continue;
        }
        let Some(identity) = document.identity.as_ref() else {
            continue;
        };
        let Some(batch_uid) = identity.parent_uid.as_ref() else {
            continue;
        };
        let filename = document.path.rsplit('/').next().unwrap_or_default();
        by_batch_and_name.insert(
            (batch_uid.clone(), filename.to_owned()),
            identity.uid.clone(),
        );
    }

    for document in &documents {
        let Some((directory, _)) = document.path.rsplit_once('/') else {
            continue;
        };
        if !registered_batches.contains(directory) {
            continue;
        }
        let filename = document.path.rsplit('/').next().unwrap_or_default();
        if filename == "README.md" || filename == "specification.md" {
            continue;
        }
        if let Some(error) = &document.metadata_error {
            anyhow::bail!(
                "Task record migration conflict at {}: {error}",
                document.path
            );
        }
        let identity = document.identity.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Task record migration conflict at {}: the task has no stable identity",
                document.path
            )
        })?;
        let existing_state = match &document.task_state {
            Some(state) => Some(state.clone()),
            None => super::load(store, &identity.uid)?,
        };
        let metadata = match &document.metadata {
            Some(metadata) => metadata.clone(),
            None => {
                let dependencies = if let Some(state) = &existing_state {
                    anyhow::ensure!(
                        identity.parent_uid.as_deref() == Some(state.batch_uid.as_str()),
                        "Task state batch UID does not match task identity at {}",
                        document.path
                    );
                    state.dependency_uids.clone()
                } else {
                    let batch_uid = identity.parent_uid.as_ref().ok_or_else(|| {
                        anyhow::anyhow!(
                            "Task record migration conflict at {}: the task has no batch identity",
                            document.path
                        )
                    })?;
                    super::super::metadata::legacy_dependencies(&document.text)?
                        .into_iter()
                        .map(|dependency| {
                            by_batch_and_name
                                .get(&(batch_uid.clone(), dependency.clone()))
                                .cloned()
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "Task record migration conflict at {}: dependency {dependency} has no task identity in the same batch",
                                        document.path
                                    )
                                })
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?
                };
                let repository_id = existing_state
                    .as_ref()
                    .map_or("root", |state| state.repository_id.as_str());
                super::super::TaskMetadata::new(identity, repository_id, dependencies)?
            }
        };

        if document.metadata.is_none() {
            let original = String::from_utf8(store.read(&document.path)?)?;
            let normalized = super::super::metadata::embed(&original, &metadata)?;
            changes.push((document.path.clone(), normalized.into_bytes()));
        }
        if existing_state.is_none() {
            let (path, bytes, check) = super::create(&metadata)?;
            changes.push((path, bytes));
            checks.push(check);
        }
    }

    Ok((expected_revision, changes, checks))
}
