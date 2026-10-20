use super::super::state::PlannerState;
use crate::core::lifecycle::{LifecycleStore, TaskPhase, TaskRecord};
use sha2::Digest;

pub(super) mod readiness;

pub(super) fn record_task_batch(
    state: &mut PlannerState,
    batch: &crate::core::workflow::TaskBatch,
) -> anyhow::Result<()> {
    let Some((feature_id, markdown)) = state.active_feature.as_ref() else {
        // Older planning flows create task batches without a feature artifact.
        // They remain supported while lifecycle-backed approval is opt-in.
        anyhow::ensure!(
            batch.feature_id.is_none(),
            "approved task batch has no active feature identity"
        );
        return Ok(());
    };
    anyhow::ensure!(
        batch.feature_id.as_deref() == Some(feature_id.as_str()),
        "task batch feature does not match the active approved feature"
    );
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)?
        .ok_or_else(|| anyhow::anyhow!("approved feature has no stable planning identity"))?;
    let workflow_batch = state
        .workflow
        .task_batches
        .last()
        .ok_or_else(|| anyhow::anyhow!("persisted task batch reference is missing"))?;
    let batch_identity = workflow_batch
        .identity
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("persisted task batch identity is missing"))?;
    anyhow::ensure!(
        batch_identity.parent_uid.as_deref() == Some(identity.uid.as_str()),
        "persisted task batch is not linked to the approved feature identity"
    );

    let lifecycle = LifecycleStore::new(&state.planning_store);
    let feature = match lifecycle.load_feature(&identity.uid) {
        Ok(feature) => feature,
        Err(crate::artifacts::planning_store::StoreError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound =>
        {
            // Legacy approved feature documents have no normalized lifecycle
            // record. The planner accepts those through the compatibility
            // path, so do not reject the task batch after writing its files.
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    };
    let plan_hash = feature
        .approved_plan_sha256
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("lifecycle feature has no approved plan receipt"))?;
    anyhow::ensure!(
        feature.plan_snapshot_uid.is_some()
            && feature.plan_snapshot_sha256.as_deref() == Some(plan_hash)
            && !feature.decision_refs.is_empty()
            && !feature.review_refs.is_empty(),
        "lifecycle feature is missing exact snapshot, user approval, or PlanReviewer evidence"
    );
    let snapshot = crate::core::lifecycle::plan_snapshot::load_persisted(
        &state.planning_store,
        feature
            .plan_snapshot_uid
            .as_deref()
            .expect("checked snapshot UID"),
    )
    .map_err(anyhow::Error::msg)?
    .ok_or_else(|| anyhow::anyhow!("approved task plan snapshot is missing"))?;
    anyhow::ensure!(
        snapshot.feature_uid == identity.uid && snapshot.content_hash == plan_hash,
        "approved task plan snapshot does not match the approved feature identity and hash"
    );
    let approved_requirement_ids = snapshot
        .requirements
        .iter()
        .map(|requirement| requirement.stable_id.as_str())
        .collect::<std::collections::HashSet<_>>();

    let documents = crate::artifacts::task_docs::load_board(&state.planning_store, &state.workflow);
    let documents = documents
        .into_iter()
        .filter(|document| {
            document
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.batch_uid == batch_identity.uid)
        })
        .collect::<Vec<_>>();
    anyhow::ensure!(
        documents.len() == batch.stories.len(),
        "persisted task batch is missing one or more validated task documents"
    );

    let namespace = uuid::Uuid::parse_str(&identity.uid)?;
    let story_indexes = batch
        .stories
        .iter()
        .enumerate()
        .map(|(index, story)| (story.title.as_str(), index))
        .collect::<std::collections::BTreeMap<_, _>>();
    anyhow::ensure!(
        story_indexes.len() == batch.stories.len(),
        "approved task batch contains duplicate story titles"
    );
    let mut documents_by_story = vec![None; batch.stories.len()];
    for document in documents {
        let metadata = document
            .metadata
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("persisted task metadata is missing"))?;
        anyhow::ensure!(
            document
                .identity
                .as_ref()
                .map(|identity| identity.uid.as_str())
                == Some(metadata.uid.as_str()),
            "persisted task identity does not match its execution metadata"
        );
        let story_title = document
            .identity
            .as_ref()
            .map(|identity| identity.title.as_str())
            .unwrap_or(document.title.as_str());
        let story_index = *story_indexes.get(story_title).ok_or_else(|| {
            anyhow::anyhow!(
                "persisted task title {:?} is outside the approved batch {:?}",
                story_title,
                story_indexes.keys().collect::<Vec<_>>()
            )
        })?;
        anyhow::ensure!(
            documents_by_story[story_index].is_none(),
            "approved task has multiple persisted document identities"
        );
        documents_by_story[story_index] = Some(document);
    }
    anyhow::ensure!(
        documents_by_story.iter().all(Option::is_some),
        "persisted task batch is missing a story from the approved order"
    );

    let lifecycle_uid = |planning_uid: &str| {
        uuid::Uuid::new_v5(
            &namespace,
            format!("task-record:v1:{plan_hash}:{planning_uid}").as_bytes(),
        )
        .hyphenated()
        .to_string()
    };
    let story_uids = documents_by_story
        .iter()
        .map(|document| {
            let document = document.as_ref().expect("all task documents checked");
            lifecycle_uid(&document.metadata.as_ref().expect("metadata checked").uid)
        })
        .collect::<Vec<_>>();
    let planning_uids = documents_by_story
        .iter()
        .map(|document| {
            document
                .as_ref()
                .expect("all task documents checked")
                .metadata
                .as_ref()
                .expect("metadata checked")
                .uid
                .clone()
        })
        .collect::<Vec<_>>();
    let planning_to_lifecycle = planning_uids
        .iter()
        .zip(&story_uids)
        .map(|(planning_uid, lifecycle_uid)| (planning_uid.as_str(), lifecycle_uid.as_str()))
        .collect::<std::collections::BTreeMap<_, _>>();

    let mut tasks = Vec::with_capacity(documents_by_story.len());
    for (story_index, document) in documents_by_story.into_iter().enumerate() {
        let document = document.expect("all task documents checked");
        let metadata = document.metadata.as_ref().expect("metadata checked");
        let content = crate::artifacts::task_docs::visible_content(&document.text);
        let content_sha = format!(
            "{:x}",
            sha2::Sha256::digest(serde_json::to_vec(&(1_u16, plan_hash, content))?.as_slice())
        );
        let dependency_task_uids = batch.stories[story_index]
            .dependencies
            .iter()
            .map(|index| {
                story_uids
                    .get(index.saturating_sub(1))
                    .filter(|_| *index > 0 && *index <= story_uids.len())
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("approved dependency index is invalid"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let task_requirement_ids = &batch.stories[story_index].approved_requirement_ids;
        anyhow::ensure!(
            task_requirement_ids
                .iter()
                .all(|requirement| { approved_requirement_ids.contains(requirement.as_str()) }),
            "task requirement links must resolve to stable requirements in the approved plan snapshot"
        );
        let persisted_dependencies = metadata
            .dependency_uids
            .iter()
            .map(|uid| {
                planning_to_lifecycle
                    .get(uid.as_str())
                    .map(|uid| (*uid).to_owned())
                    .ok_or_else(|| {
                        anyhow::anyhow!("persisted dependency is outside the approved batch")
                    })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        anyhow::ensure!(
            persisted_dependencies == dependency_task_uids,
            "persisted dependencies differ from the approved task DAG"
        );
        let parent_task_uid = batch.stories[story_index]
            .parent_task_index
            .map(|index| {
                story_uids
                    .get(index.saturating_sub(1))
                    .filter(|_| index > 0 && index <= story_uids.len())
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("approved parent task index is invalid"))
            })
            .transpose()?;
        tasks.push(TaskRecord {
            schema_version: 1,
            task_uid: story_uids[story_index].clone(),
            feature_uid: identity.uid.clone(),
            planning_item_uid: Some(metadata.uid.clone()),
            task_spec_sha256: Some(content_sha),
            parent_task_uid,
            subtask_uids: Vec::new(),
            dependency_task_uids,
            approved_requirement_ids: task_requirement_ids.clone(),
            phase: TaskPhase::Ready,
            revision: 0,
            approved_plan_sha256: plan_hash.to_owned(),
            implementation_sha256: None,
            reviewed_code_head_sha: None,
            reviewed_code_subject_sha256: None,
            code_review: None,
            review_history: Vec::new(),
            repair_attempts: 0,
            runtime_qa: None,
            qa_execution: None,
            spec_reconciliation: None,
            spec_reconciliation_blocker: None,
            spec_reconciliation_proposal_sha256: None,
            spec_reconciliation_spec_sha256: None,
            spec_reconciliation_proposal_json: None,
            pr_work_item_uid: None,
        });
    }
    let parent_edges = tasks
        .iter()
        .filter_map(|task| {
            task.parent_task_uid
                .as_ref()
                .map(|parent_uid| (parent_uid.clone(), task.task_uid.clone()))
        })
        .collect::<Vec<_>>();
    for (parent_uid, child_uid) in parent_edges {
        let parent = tasks
            .iter_mut()
            .find(|candidate| candidate.task_uid == parent_uid)
            .ok_or_else(|| anyhow::anyhow!("approved parent task is missing"))?;
        parent.subtask_uids.push(child_uid);
    }
    lifecycle.reconcile_task_batch(&identity.uid, plan_hash, &tasks)?;
    state.baseline_planning_revision = state.planning_store.revision()?;
    Ok(())
}
