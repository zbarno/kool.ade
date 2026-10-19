use super::PlanReviewOutcome;
use crate::harness::{ExecutionMode, LiveProgress, PlanningRequest};
use std::sync::{Arc, atomic::AtomicBool, mpsc};
mod context;
#[cfg(not(test))]
pub(super) fn recover_persisted_review(
    project: &super::super::session::Project,
    feature_id: &str,
) -> Result<Option<PlanReviewOutcome>, String> {
    let text = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == feature_id)
        .map(|(_, text)| text.as_str())
        .ok_or_else(|| "confirmed feature is no longer active".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(text)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "feature identity is missing".to_owned())?;
    let contract = crate::core::workflow::feature_contract(text);
    let store = &project.state.planning_store;
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        store, feature_id, &identity, text, &contract,
    )
    .map_err(|error| error.to_string())?;
    let Some(receipt) =
        crate::core::lifecycle::plan_snapshot::plan_review_receipt(store, &snapshot)
            .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    let Some(verdict) =
        crate::core::lifecycle::plan_snapshot::plan_review_verdict(store, &snapshot)
            .map_err(|error| error.to_string())?
    else {
        return Ok(None);
    };
    if !crate::core::lifecycle::plan_snapshot::has_current_user_confirmation(store, &snapshot)
        .map_err(|error| error.to_string())?
    {
        return Err("saved plan review has no current matching user confirmation".into());
    }
    Ok(Some(PlanReviewOutcome {
        feature_id: feature_id.to_owned(),
        snapshot_uid: snapshot.uid,
        snapshot_hash: snapshot.content_hash,
        author_run_id: receipt
            .author_run_id
            .clone()
            .ok_or_else(|| "saved plan review has no author run identity".to_owned())?,
        author_session_id: receipt
            .author_session_id
            .clone()
            .ok_or_else(|| "saved plan review has no author session identity".to_owned())?,
        reviewer_run_id: receipt
            .reviewer_run_id
            .clone()
            .ok_or_else(|| "saved plan review has no reviewer run identity".to_owned())?,
        reviewer_session_id: receipt
            .reviewer_session_id
            .clone()
            .ok_or_else(|| "saved plan review has no reviewer session identity".to_owned())?,
        evidence_refs: receipt.evidence_refs.clone(),
        prompt: receipt
            .prompt_manifest
            .clone()
            .ok_or_else(|| "saved plan review has no prompt provenance".to_owned())?,
        verdict,
    }))
}
pub(super) fn prepare(
    project: &super::super::session::Project,
    feature_id: String,
) -> Result<
    (
        PlanningRequest,
        crate::harness::prompt_catalog::OutputValidationContext<'static>,
        PlanReviewOutcome,
    ),
    String,
> {
    use crate::harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1};
    let text = project
        .state
        .active_features
        .iter()
        .find(|(id, _)| id == &feature_id)
        .map(|(_, text)| text.as_str())
        .ok_or_else(|| "confirmed feature is no longer active".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(text)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "feature identity is missing".to_owned())?;
    let contract = crate::core::workflow::feature_contract(text);
    let snapshot = crate::core::lifecycle::plan_snapshot::from_feature_with_author(
        &project.state.planning_store,
        &feature_id,
        &identity,
        text,
        &contract,
    )
    .map_err(|error| error.to_string())?;
    if !crate::core::lifecycle::plan_snapshot::is_persisted(
        &project.state.planning_store,
        &snapshot,
    )
    .map_err(|error| error.to_string())?
    {
        return Err("confirmed plan snapshot is not persisted".into());
    }
    if !crate::core::lifecycle::plan_snapshot::has_current_user_confirmation(
        &project.state.planning_store,
        &snapshot,
    )
    .map_err(|error| error.to_string())?
    {
        return Err("independent plan review cannot start before current user confirmation".into());
    }
    if crate::core::lifecycle::plan_snapshot::recorded_review_decision(
        &project.state.planning_store,
        &snapshot,
    )
    .map_err(|error| error.to_string())?
    .is_some()
    {
        return Err("this snapshot already has a review receipt; resolve the Manager findings, revise the plan, and obtain fresh user approval".into());
    }
    let author_run_id = snapshot.author_run_id.clone().ok_or_else(|| {
        "confirmed plan has no persisted author run provenance; independent review is blocked"
            .to_owned()
    })?;
    let reviewer_run_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let author_session_id = snapshot
        .author_session_id
        .clone()
        .ok_or_else(|| "confirmed plan has no persisted author session provenance; independent review is blocked".to_owned())?;
    let reviewer_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let role = PromptRole::PlanReviewer;
    let review_context = context::build(project, &feature_id, &snapshot)?;
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let rendered = crate::harness::prompt_catalog::render(&RoleInputV1 {
        role,
        workspace_uid: workspace_uid.clone(),
        feature_uid: identity.uid.clone(),
        task_uid: None,
        original_goal: review_context.original_goal.clone(),
        approved_spec_ref: Some(snapshot.uid.clone()),
        approved_spec_hash: Some(snapshot.content_hash.clone()),
        decision_refs: review_context.decision_refs,
        evidence_manifest: review_context.evidence_manifest,
        execution_policy_ref: "plan-review/read-only/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: snapshot.spec_revision.clone(),
            subject_hash: snapshot.content_hash.clone(),
            author_run_id: author_run_id.clone(),
            reviewer_run_id: reviewer_run_id.clone(),
            author_session_id: Some(author_session_id.clone()),
            reviewer_session_id: Some(reviewer_session_id.clone()),
            requirements_matrix: snapshot
                .requirements
                .iter()
                .map(|req| {
                    format!(
                        "{}: {} | rationale: {} | acceptance criteria: {}",
                        req.stable_id,
                        req.text,
                        req.rationale
                            .as_deref()
                            .unwrap_or("not stated in the specification"),
                        if req.ac_ids.is_empty() {
                            "unmapped".to_owned()
                        } else {
                            req.ac_ids.join(", ")
                        }
                    )
                })
                .collect(),
            explicit_exclusions: snapshot.non_goals.clone(),
        },
    })
    .map_err(|error| error.to_string())?;
    let evidence_ids = review_context.evidence_ids.clone();
    let evidence_ids: std::collections::HashSet<String> = evidence_ids.iter().cloned().collect();
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        expected_subject_hash: None,
        evidence_ids: evidence_ids.clone(),
        required_requirements: snapshot
            .requirements
            .iter()
            .map(|req| req.stable_id.clone())
            .collect(),
        ..Default::default()
    };
    let (progress_tx, _progress_rx) = mpsc::channel::<LiveProgress>();
    let request = PlanningRequest {
        mode: ExecutionMode::ReadOnlyAnalysis,
        task_id: None,
        execution_workspace: None,
        destination_sha: None,
        prompt_manifest: Some(rendered.manifest.clone()),
        reasoning_level: "high".into(),
        telemetry_phase: Some("independent_plan_review".into()),
        repo_root: project.state.repo_root.clone(),
        runtime_config_source: None,
        prompt_body: rendered.user,
        system_instructions: rendered.system,
        timeout: crate::core::turn::configured_turn_timeout(),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),

        authorization_verifier: None,
        review_identity: Some(crate::harness::ReviewExecutionIdentity {
            execution_id: reviewer_run_id.clone(),
            session_id: reviewer_session_id.clone(),
        }),
    };
    Ok((
        request,
        context,
        PlanReviewOutcome {
            feature_id,
            snapshot_uid: snapshot.uid,
            snapshot_hash: snapshot.content_hash,
            author_run_id,
            author_session_id,
            reviewer_run_id,
            reviewer_session_id,
            evidence_refs: evidence_ids.into_iter().collect(),
            prompt: rendered.manifest,
            verdict: crate::harness::prompt_catalog::ReviewVerdictV1 {
                subject_hash: String::new(),
                pass: false,
                coverage: Vec::new(),
                findings: Vec::new(),
                untested: Vec::new(),
            },
        },
    ))
}
