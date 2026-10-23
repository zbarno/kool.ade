use super::{Implementation, TaskRepositoryKind};
use crate::{
    artifacts::{planning_store::PlanningStore, task_docs::TaskMetadata},
    core::{
        lifecycle::LifecycleStore,
        project_repos,
        self_improvement::{SelfImprovementStore, TaskSourceAuthorization},
    },
};
use std::sync::Arc;

#[cfg(test)]
#[path = "self_improvement/tests.rs"]
mod tests;

/// Revalidate persisted self-improvement source authorization after the
/// ordinary implementation runner has resolved its actual task clone.
pub(super) fn authorize_resolved_task_source(
    planning: &PlanningStore,
    state_root: &std::path::Path,
    state_path: std::path::PathBuf,
    identity_uid: Option<&str>,
    metadata: Option<&TaskMetadata>,
    implementation: &Implementation,
    prior_verifier: Option<Arc<dyn crate::execution::session::AuthorizationGenerationVerifier>>,
) -> anyhow::Result<Option<Arc<dyn crate::execution::session::AuthorizationGenerationVerifier>>> {
    let Some(planning_item_uid) = metadata
        .map(|metadata| metadata.uid.as_str())
        .or(identity_uid)
    else {
        return Ok(prior_verifier);
    };
    let lifecycle = LifecycleStore::new(planning);
    let Some(task) = lifecycle.task_for_planning_item(planning_item_uid)? else {
        return Ok(prior_verifier);
    };
    let runs = SelfImprovementStore::new(planning);
    let Some(run) = runs.run_for_feature(&task.feature_uid)? else {
        // Reject linked product tasks whose Pursue run has disappeared.
        runs.authorize_task_start(&task.feature_uid, Some(&task.task_uid), "", "")?;
        return Ok(prior_verifier);
    };
    verify_plan_binding(planning, &run, &task.feature_uid, &task.task_uid)?;
    let manifest = project_repos::ProjectManifest::load(planning)?;
    let workspace_uid = planning.project_id.hyphenated().to_string();
    let repository = manifest
        .repositories
        .iter()
        .find(|repository| {
            project_repos::stable_uid(&workspace_uid, "repository", &repository.id)
                == run.target_repository_uid
        })
        .ok_or_else(|| {
            anyhow::anyhow!("Self-improvement target is missing from the repository manifest")
        })?;
    anyhow::ensure!(
        implementation.repository_id.as_deref() == Some(run.target_repository_uid.as_str()),
        "Resolved implementation repository differs from the approved self-improvement target"
    );
    runs.authorize_task_source(TaskSourceAuthorization {
        feature_uid: &task.feature_uid,
        task_uid: &task.task_uid,
        repository_uid: &run.target_repository_uid,
        repository_remote: &repository.remote,
        source_ref: implementation.source_ref.as_deref(),
        source_sha: implementation.source_commit.as_deref(),
        destination_ref: implementation.destination_branch.as_deref(),
        // The runner has allocated the clone path, but creates its contents
        // during reconciliation. The session verifier checks readiness before
        // the first Docker operation.
        isolated_clone: implementation.task_repository_kind == TaskRepositoryKind::Clone,
    })?;
    let generation = execution_generation(&run.run_uid, run.revision)?;
    let prior_generation = prior_verifier
        .as_ref()
        .map(|verifier| verifier.generation());
    Ok(Some(Arc::new(SelfImprovementExecutionVerifier {
        planning: planning.clone(),
        state_root: state_root.to_path_buf(),
        state_path,
        planning_item_uid: planning_item_uid.to_owned(),
        expected_run_uid: run.run_uid,
        expected_generation: generation,
        feature_uid: task.feature_uid,
        task_uid: task.task_uid,
        repository_uid: run.target_repository_uid,
        repository_remote: repository.remote.clone(),
        source_ref: implementation.source_ref.clone(),
        source_sha: implementation.source_commit.clone(),
        destination_ref: implementation.destination_branch.clone(),
        prior_verifier,
        prior_generation,
    })))
}

fn execution_generation(run_uid: &str, revision: u64) -> Result<String, uuid::Error> {
    let run_namespace = uuid::Uuid::parse_str(run_uid)?;
    Ok(uuid::Uuid::new_v5(
        &run_namespace,
        format!("self-improvement-execution:{revision}").as_bytes(),
    )
    .hyphenated()
    .to_string())
}

fn verify_plan_binding(
    planning: &PlanningStore,
    run: &crate::core::self_improvement::SelfImprovementRunV1,
    feature_uid: &str,
    task_uid: &str,
) -> anyhow::Result<()> {
    let lifecycle = LifecycleStore::new(planning);
    let feature = lifecycle.load_feature(feature_uid)?;
    let task = lifecycle.load_task(task_uid)?;
    let plan_hash = run.plan_approval_hash.as_deref();
    let snapshot_uid = feature.plan_snapshot_uid.as_deref();
    let snapshot = snapshot_uid
        .map(|uid| crate::core::lifecycle::plan_snapshot::load_persisted(planning, uid))
        .transpose()?
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("Persisted approved plan snapshot is missing"))?;
    let user_receipt =
        crate::core::lifecycle::plan_snapshot::user_confirmation_receipt(planning, &snapshot)?
            .ok_or_else(|| anyhow::anyhow!("Persisted user plan confirmation is missing"))?;
    let current_confirmation =
        crate::core::lifecycle::plan_snapshot::has_current_user_confirmation(planning, &snapshot)?;
    let review_receipt =
        crate::core::lifecycle::plan_snapshot::plan_review_receipt(planning, &snapshot)?
            .ok_or_else(|| anyhow::anyhow!("Persisted independent plan review is missing"))?;
    let approved_review =
        crate::core::lifecycle::plan_snapshot::has_approved_review(planning, &snapshot)?;
    require_approved_review_evidence(
        approved_review,
        &review_receipt.receipt_uid,
        &feature.review_refs,
    )?;
    anyhow::ensure!(
        run.feature_uid == feature_uid
            && task.feature_uid == feature_uid
            && feature.phase != crate::core::lifecycle::FeaturePhase::Cancelled
            && task.phase != crate::core::lifecycle::TaskPhase::Cancelled
            && plan_hash.is_some()
            && snapshot.feature_uid == feature_uid
            && Some(snapshot.content_hash.as_str()) == plan_hash
            && snapshot.capability_id.is_some()
            && run.capability_id.as_deref() == snapshot.capability_id.as_deref()
            && feature.approved_plan_sha256.as_deref() == plan_hash
            && feature.plan_snapshot_sha256.as_deref() == plan_hash
            && Some(task.approved_plan_sha256.as_str()) == plan_hash
            && user_receipt.actor == crate::core::lifecycle::GateActor::User
            && user_receipt.decision == crate::core::lifecycle::GateDecision::Approved
            && user_receipt.subject_uid == snapshot.uid
            && user_receipt.subject_sha256 == snapshot.content_hash
            && current_confirmation
            && feature.decision_refs.len() == 1
            && feature.decision_refs.first() == Some(&user_receipt.receipt_uid)
            && review_receipt.actor == crate::core::lifecycle::GateActor::IndependentReviewer
            && review_receipt.decision == crate::core::lifecycle::GateDecision::Approved
            && review_receipt.prompt_role()
                == Some(crate::harness::prompt_catalog::PromptRole::PlanReviewer),
        "Lifecycle feature, task, and persisted self-improvement approval do not match"
    );
    Ok(())
}

fn require_approved_review_evidence(
    approved_review: bool,
    receipt_uid: &str,
    review_refs: &[String],
) -> anyhow::Result<()> {
    anyhow::ensure!(
        approved_review && review_refs.iter().any(|reference| reference == receipt_uid),
        "Persisted PlanReviewer verdict is missing, rejected, or unlinked"
    );
    Ok(())
}

#[derive(Debug)]
struct SelfImprovementExecutionVerifier {
    planning: PlanningStore,
    state_root: std::path::PathBuf,
    state_path: std::path::PathBuf,
    planning_item_uid: String,
    expected_run_uid: String,
    expected_generation: String,
    feature_uid: String,
    task_uid: String,
    repository_uid: String,
    repository_remote: String,
    source_ref: Option<String>,
    source_sha: Option<String>,
    destination_ref: Option<String>,
    prior_verifier: Option<Arc<dyn crate::execution::session::AuthorizationGenerationVerifier>>,
    prior_generation: Option<String>,
}

impl crate::execution::session::AuthorizationGenerationVerifier
    for SelfImprovementExecutionVerifier
{
    fn generation(&self) -> String {
        self.expected_generation.clone()
    }

    fn verify(&self, generation: &str) -> Result<(), crate::execution::errors::ExecutionError> {
        use crate::execution::errors::{ExecutionError, ExecutionErrorKind};

        let denied =
            |message: String| ExecutionError::new(ExecutionErrorKind::PolicyDenied, message, false);
        if generation != self.expected_generation {
            return Err(denied(
                "Self-improvement execution authorization generation changed".into(),
            ));
        }
        if let (Some(prior), Some(prior_generation)) =
            (&self.prior_verifier, self.prior_generation.as_deref())
        {
            prior
                .verify(prior_generation)
                .map_err(|error| denied(error.message))?;
        }
        let runs = SelfImprovementStore::new(&self.planning);
        let current = runs
            .run_for_feature(&self.feature_uid)
            .map_err(|error| denied(error.to_string()))?
            .ok_or_else(|| denied("Persisted self-improvement authorization disappeared".into()))?;
        if current.run_uid != self.expected_run_uid {
            return Err(denied(
                "Persisted self-improvement authorization changed run identity".into(),
            ));
        }
        verify_plan_binding(&self.planning, &current, &self.feature_uid, &self.task_uid)
            .map_err(|error| denied(error.to_string()))?;
        let implementation =
            super::read_state_file(&self.state_path).map_err(|error| denied(error.to_string()))?;
        if implementation.task_uid.as_deref() != Some(self.planning_item_uid.as_str())
            || implementation.repository_id.as_deref() != Some(self.repository_uid.as_str())
            || implementation.source_ref != self.source_ref
            || implementation.source_commit != self.source_sha
            || implementation.destination_branch != self.destination_ref
            || implementation.task_repository_kind != TaskRepositoryKind::Clone
            || !implementation.task_repository_ready
        {
            return Err(denied(
                "Persisted implementation state is not the approved ready task clone".into(),
            ));
        }
        super::task_repository::validate_task_path(&self.state_root, &implementation)
            .map_err(|error| denied(error.to_string()))?;
        runs.authorize_task_source(TaskSourceAuthorization {
            feature_uid: &self.feature_uid,
            task_uid: &self.task_uid,
            repository_uid: &self.repository_uid,
            repository_remote: &self.repository_remote,
            source_ref: self.source_ref.as_deref(),
            source_sha: self.source_sha.as_deref(),
            destination_ref: self.destination_ref.as_deref(),
            isolated_clone: true,
        })
        .map_err(|error| denied(error.to_string()))
    }
}
