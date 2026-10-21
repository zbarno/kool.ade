use crate::{
    artifacts::planning_store::PlanningStore,
    execution::contract::WorkspaceRef,
    project_runtime::{
        ProjectRuntimeStore, RuntimeOperation, RuntimeReceiptState, RuntimeReceiptV1,
    },
};

pub(super) fn fail_unfinished_claims(
    planning_store: &PlanningStore,
    workspace: &WorkspaceRef,
    target_id: &str,
    operation: RuntimeOperation,
    recipe_ids: &[String],
) -> Result<(), String> {
    let store = ProjectRuntimeStore::new(planning_store);
    let receipts = store
        .receipts_for_task(&workspace.task_id)
        .map_err(|error| error.to_string())?;
    let mut observed_at = receipts
        .iter()
        .map(|receipt| receipt.observed_at_unix_ms)
        .max()
        .unwrap_or_default()
        .saturating_add(1)
        .max(super::candidate::now_unix_ms());
    for recipe_id in recipe_ids {
        let Some(latest) = super::candidate::latest_receipt(
            &receipts,
            operation,
            target_id,
            &workspace.source_sha,
            recipe_id,
        ) else {
            continue;
        };
        if latest.state != RuntimeReceiptState::Running {
            continue;
        }
        store
            .append_receipt(&RuntimeReceiptV1 {
                schema_version: 1,
                receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
                operation,
                task_id: workspace.task_id.clone(),
                target_id: target_id.to_owned(),
                source_sha: workspace.source_sha.clone(),
                recipe_id: Some(recipe_id.clone()),
                state: RuntimeReceiptState::Failed,
                exit_code: None,
                artifact_hashes: Default::default(),
                prepared_dependency_ids: Vec::new(),
                log_reference: None,
                observed_at_unix_ms: observed_at,
            })
            .map_err(|error| error.to_string())?;
        observed_at = observed_at.saturating_add(1);
    }
    Ok(())
}

#[cfg(test)]
fn receipt(recipe_id: &str, state: RuntimeReceiptState, at: u64) -> RuntimeReceiptV1 {
    RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        operation: RuntimeOperation::Build,
        task_id: "task-1".into(),
        target_id: "web".into(),
        source_sha: "a".repeat(40),
        recipe_id: Some(recipe_id.into()),
        state,
        exit_code: (state == RuntimeReceiptState::Passed).then_some(0),
        artifact_hashes: Default::default(),
        prepared_dependency_ids: Vec::new(),
        log_reference: None,
        observed_at_unix_ms: at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalizes_only_latest_running_receipts_for_claimed_recipes() {
        let root = std::env::temp_dir().join(format!(
            "koolade-runtime-recipe-completion-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let planning = PlanningStore::legacy_embedded(uuid::Uuid::new_v4(), &root);
        let store = ProjectRuntimeStore::new(&planning);
        let workspace = WorkspaceRef {
            project_id: "project".into(),
            task_id: "task-1".into(),
            source_sha: "a".repeat(40),
        };
        store
            .append_receipt(&receipt("build-v1", RuntimeReceiptState::Running, 1))
            .unwrap();
        store
            .append_receipt(&receipt("test-v1", RuntimeReceiptState::Passed, 2))
            .unwrap();

        fail_unfinished_claims(
            &planning,
            &workspace,
            "web",
            RuntimeOperation::Build,
            &["build-v1".into(), "test-v1".into()],
        )
        .unwrap();

        let receipts = store.receipts_for_task("task-1").unwrap();
        assert_eq!(
            crate::project_runtime::RuntimeReceiptState::Failed,
            super::super::candidate::latest_receipt(
                &receipts,
                RuntimeOperation::Build,
                "web",
                &workspace.source_sha,
                "build-v1",
            )
            .unwrap()
            .state
        );
        assert_eq!(
            RuntimeReceiptState::Passed,
            super::super::candidate::latest_receipt(
                &receipts,
                RuntimeOperation::Build,
                "web",
                &workspace.source_sha,
                "test-v1",
            )
            .unwrap()
            .state
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
