use crate::core::{
    state::PlannerState,
    workflow::{InterviewBrief, TaskOutline, TaskStory},
};

use super::run::Run;

pub(super) fn run(
    run: &Run,
    harness: &dyn crate::harness::AiHarness,
    request: &crate::harness::PlanningRequest,
    state: &PlannerState,
    snapshot: &crate::core::lifecycle::PlanSnapshotV1,
    brief: &InterviewBrief,
) -> anyhow::Result<(Vec<TaskOutline>, Vec<TaskStory>)> {
    use crate::harness::ExecutionMode;
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1};
    let identity = crate::domain::ArtifactIdentity::from_markdown(
        &state
            .active_feature
            .as_ref()
            .expect("approved feature checked")
            .1,
    )?
    .ok_or_else(|| anyhow::anyhow!("Approved feature has no identity"))?;
    let lifecycle = crate::core::lifecycle::LifecycleStore::new(&state.planning_store);
    let goal = lifecycle
        .load_feature(&identity.uid)
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let plan_hash = &snapshot.content_hash;
    if !(goal.phase == crate::core::lifecycle::FeaturePhase::ReadyToDecompose
        && goal.plan_snapshot_uid.as_deref() == Some(snapshot.uid.as_str())
        && goal.plan_snapshot_sha256.as_deref() == Some(plan_hash.as_str())
        && goal.approved_plan_sha256.as_deref() == Some(plan_hash.as_str())
        && crate::core::lifecycle::plan_snapshot::is_persisted(&state.planning_store, snapshot)?
        && crate::core::lifecycle::plan_snapshot::has_current_user_confirmation(
            &state.planning_store,
            snapshot,
        )?
        && crate::core::lifecycle::plan_snapshot::has_approved_review(
            &state.planning_store,
            snapshot,
        )?)
    {
        anyhow::bail!(
            "Current immutable user-approved plan and independent review are required before decomposition"
        );
    }
    let branches = state
        .active_feature
        .as_ref()
        .and_then(|(id, _)| state.workflow.feature_branch_targets.get(id))
        .ok_or_else(|| anyhow::anyhow!("Approved task branch targets are missing"))?;
    let mut allowed_repositories = std::collections::HashMap::new();
    let mut approved_branch_targets = std::collections::HashMap::new();
    let mut repository_source_shas = std::collections::HashMap::new();
    let mut repository_context = Vec::new();
    for repo in &state.repositories.repositories {
        let checkout = state
            .repositories
            .target(&state.repo_root, &repo.id)
            .map_err(|error| {
                anyhow::anyhow!("Repository {} has no trusted checkout: {error}", repo.id)
            })?;
        let sha = source_sha(&checkout, &branches.source)?;
        allowed_repositories.insert(repo.id.clone(), repo.remote.clone());
        approved_branch_targets.insert(
            repo.id.clone(),
            (branches.source.clone(), branches.destination.clone()),
        );
        repository_source_shas.insert(repo.id.clone(), sha);
        repository_context.push(serde_json::json!({
            "repository_id": repo.id,
            "portable_identity": repo.remote,
            "source_branch": branches.source,
            "source_sha": repository_source_shas[&repo.id],
            "destination_branch": branches.destination,
        }));
    }
    let decision_refs = goal.decision_refs.clone();
    let input = RoleInputV1 {
        role: PromptRole::Decomposer,
        workspace_uid: uuid::Uuid::new_v5(
            &uuid::Uuid::parse_str(&identity.uid).expect("validated artifact UUID"),
            b"koolade-task-generation-workspace-v1",
        )
        .hyphenated()
        .to_string(),
        feature_uid: identity.uid.clone(),
        task_uid: None,
        original_goal: snapshot.intent.clone(),
        approved_spec_ref: Some(snapshot.uid.clone()),
        approved_spec_hash: Some(snapshot.content_hash.clone()),
        decision_refs: decision_refs.clone(),
        evidence_manifest: Vec::new(),
        execution_policy_ref: "application:approved-task-generation".into(),
        output_schema_version: PromptRole::Decomposer.output_schema_version().into(),
        payload: RolePayloadV1::Decomposer {
            approved_plan_hash: snapshot.content_hash.clone(),
            approved_scope: serde_json::to_string(&serde_json::json!({
                "approved_plan": snapshot,
                "approved_repositories": repository_context,
            }))
            .unwrap_or_default(),
        },
    };
    let rendered = crate::harness::prompt_catalog::render(&input).map_err(anyhow::Error::msg)?;
    let mut role_request = request.clone();
    role_request.mode = ExecutionMode::ReadOnlyAnalysis;
    role_request.prompt_manifest = Some(rendered.manifest);
    role_request.prompt_body = rendered.user.clone();
    role_request.system_instructions = rendered.system;
    let requirement_ids = snapshot
        .requirements
        .iter()
        .map(|item| item.stable_id.clone())
        .collect::<Vec<_>>();
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        expected_feature_uid: Some(&identity.uid),
        expected_approved_spec_ref: Some(&snapshot.uid),
        expected_approved_plan_hash: Some(plan_hash),
        approved_decision_refs: decision_refs,
        required_requirements: requirement_ids,
        allowed_repositories,
        approved_branch_targets,
        repository_source_shas,
        ..Default::default()
    };
    let graph = run
        .validated(
            harness,
            &role_request,
            "Approved task decomposition",
            6,
            rendered.user,
            |text| match crate::harness::prompt_catalog::validate_output_with_context(
                PromptRole::Decomposer,
                text,
                &context,
            ) {
                Ok(crate::harness::prompt_catalog::RoleOutputV1::TaskGraph(graph)) => Ok(graph),
                Ok(_) => Err(vec!["Decomposer returned the wrong output type".into()]),
                Err(error) => Err(vec![error]),
            },
        )
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    flatten(graph, snapshot, brief)
}

pub(super) fn checkpoint_matches(state: &PlannerState, stories: &[TaskStory]) -> bool {
    let Some(branches) = state
        .active_feature
        .as_ref()
        .and_then(|(id, _)| state.workflow.feature_branch_targets.get(id))
    else {
        return false;
    };
    stories.iter().all(|story| {
        let (Some(_), Some(expected_sha)) = (story.graph_uid.as_ref(), story.source_sha.as_deref())
        else {
            return false;
        };
        let Ok(checkout) = state
            .repositories
            .target(&state.repo_root, &story.target_repository)
        else {
            return false;
        };
        source_sha(&checkout, &branches.source).is_ok_and(|current| current == expected_sha)
    })
}

fn source_sha(checkout: &std::path::Path, branch: &str) -> anyhow::Result<String> {
    let local_ref = format!("refs/heads/{branch}^{{commit}}");
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--verify", &local_ref])
        .current_dir(checkout)
        .output()?;
    let output = if output.status.success() {
        output
    } else {
        let remote_ref = format!("refs/remotes/origin/{branch}^{{commit}}");
        std::process::Command::new("git")
            .args(["rev-parse", "--verify", &remote_ref])
            .current_dir(checkout)
            .output()?
    };
    anyhow::ensure!(
        output.status.success(),
        "Source branch {branch} has no resolvable commit"
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub(super) use flatten::flatten;

mod flatten;

#[cfg(test)]
#[path = "decompose/tests.rs"]
mod tests;
