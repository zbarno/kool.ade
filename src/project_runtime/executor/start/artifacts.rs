use std::collections::BTreeMap;

use crate::{
    execution::session::ExecutionSession,
    project_runtime::{
        AppTargetV1, ProjectCapabilityManifestV1, ProjectRuntimeStore, RuntimeOperation,
        RuntimeReceiptState, RuntimeReceiptV1,
    },
};

pub(super) struct BuildArtifacts {
    pub paths: Vec<String>,
    pub hashes: BTreeMap<String, String>,
}

pub(super) struct ReceiptData {
    pub recipe_id: String,
    pub exit_code: Option<i32>,
    pub log_reference: Option<String>,
    pub artifact_hashes: BTreeMap<String, String>,
    pub passed: bool,
    pub observed_at_unix_ms: u64,
}

pub(super) fn build_artifacts(
    manifest: &ProjectCapabilityManifestV1,
    target: &AppTargetV1,
    receipts: &[RuntimeReceiptV1],
    source_sha: &str,
) -> anyhow::Result<Option<BuildArtifacts>> {
    let Some(build_recipe_id) = target.build_recipe_ref.as_deref() else {
        return Ok(None);
    };
    let Some(build) = receipts
        .iter()
        .filter(|receipt| {
            receipt.operation == RuntimeOperation::Build
                && receipt.task_id == target.owning_task_id
                && receipt.target_id == target.target_id
                && receipt.source_sha == source_sha
                && receipt.recipe_id.as_deref() == Some(build_recipe_id)
        })
        .max_by_key(|receipt| (receipt.observed_at_unix_ms, receipt.receipt_id.clone()))
    else {
        return Ok(None);
    };
    anyhow::ensure!(
        build.state == RuntimeReceiptState::Passed && build.exit_code == Some(0),
        "latest runtime Build receipt is not passing for the exact reviewed source"
    );
    let artifact_paths = if let Some(reference) = target.artifact_spec_ref.as_ref() {
        manifest
            .artifact_specs
            .get(reference)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("runtime artifact specification is missing"))?
    } else {
        manifest
            .recipes
            .iter()
            .chain(manifest.user_overrides.iter())
            .find(|recipe| recipe.recipe_id == build_recipe_id)
            .ok_or_else(|| anyhow::anyhow!("runtime build recipe is missing"))?
            .expected_artifacts
            .clone()
    };
    let workspace_paths = artifact_paths
        .iter()
        .map(|path| super::super::join_workspace_path(&target.project_relative_root, path))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let hashes = build.artifact_hashes.clone();
    anyhow::ensure!(
        hashes.len() == workspace_paths.len()
            && workspace_paths.iter().all(|path| hashes.contains_key(path)),
        "successful build receipt does not match the approved artifact specification"
    );
    Ok(Some(BuildArtifacts {
        paths: artifact_paths,
        hashes,
    }))
}

pub(super) fn failed_start(
    store: &ProjectRuntimeStore<'_>,
    session: &ExecutionSession,
    target: &AppTargetV1,
    recipe_id: String,
) -> anyhow::Result<Vec<RuntimeReceiptV1>> {
    let receipt = receipt(
        session,
        target,
        ReceiptData {
            recipe_id,
            exit_code: None,
            log_reference: None,
            artifact_hashes: BTreeMap::new(),
            passed: false,
            observed_at_unix_ms: super::super::next_receipt_timestamp(
                store,
                &session.workspace().task_id,
            )?,
        },
    );
    store.append_receipt(&receipt)?;
    Ok(vec![receipt])
}

pub(super) fn receipt(
    session: &ExecutionSession,
    target: &AppTargetV1,
    data: ReceiptData,
) -> RuntimeReceiptV1 {
    RuntimeReceiptV1 {
        schema_version: 1,
        receipt_id: uuid::Uuid::new_v4().hyphenated().to_string(),
        operation: RuntimeOperation::Start,
        task_id: session.workspace().task_id.clone(),
        target_id: target.target_id.clone(),
        source_sha: session.workspace().source_sha.clone(),
        recipe_id: Some(data.recipe_id),
        state: if data.passed {
            RuntimeReceiptState::Passed
        } else {
            RuntimeReceiptState::Failed
        },
        exit_code: data.exit_code,
        artifact_hashes: data.artifact_hashes,
        prepared_dependency_ids: session.prepared_dependency_ids(),
        log_reference: data.log_reference,
        observed_at_unix_ms: data.observed_at_unix_ms,
    }
}
