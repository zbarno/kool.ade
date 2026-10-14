//! Bounded independent review of a persisted, user-confirmed plan snapshot.
#[cfg(not(test))]
use crate::harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub(crate) struct PlanReview {
    result: mpsc::Receiver<Result<PlanReviewOutcome, String>>,
    cancel: Arc<AtomicBool>,
}

pub(crate) struct PlanReviewOutcome {
    pub feature_id: String,
    pub snapshot_uid: String,
    pub snapshot_hash: String,
    pub author_run_id: String,
    pub author_session_id: String,
    pub reviewer_run_id: String,
    pub reviewer_session_id: String,
    pub prompt: crate::harness::prompt_catalog::PromptManifest,
    pub verdict: crate::harness::prompt_catalog::ReviewVerdictV1,
}

impl PlanReview {
    #[cfg(not(test))]
    pub(crate) fn start(
        project: &super::session::Project,
        feature_id: String,
        harness: Box<dyn AiHarness>,
    ) -> Result<Self, String> {
        let (mut request, context, outcome) = prepare(project, feature_id)?;
        let (done, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        request.cancel = worker_cancel.clone();
        std::thread::spawn(move || {
            let result = if worker_cancel.load(Ordering::Relaxed) {
                Err("plan review was cancelled".into())
            } else {
                harness
                    .execute_review(
                        &request,
                        request
                            .review_identity
                            .as_ref()
                            .expect("review identity set"),
                    )
                    .map_err(|error| error.detail())
                    .and_then(|response| {
                        let expected_hash = outcome.snapshot_hash.clone();
                        let mut context = context;
                        context.expected_subject_hash = Some(&expected_hash);
                        match crate::harness::prompt_catalog::validate_output_with_context(
                            crate::harness::prompt_catalog::PromptRole::PlanReviewer,
                            &response.final_text,
                            &context,
                        )? {
                            crate::harness::prompt_catalog::RoleOutputV1::Review(verdict) => {
                                Ok(PlanReviewOutcome { verdict, ..outcome })
                            }
                            _ => Err("plan reviewer returned an unexpected schema".into()),
                        }
                    })
            };
            let _ = done.send(result);
        });
        Ok(Self { result, cancel })
    }

    pub(crate) fn take_result(&self) -> Option<Result<PlanReviewOutcome, String>> {
        self.result.try_recv().ok()
    }
}

impl Drop for PlanReview {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(not(test))]
fn prepare(
    project: &super::session::Project,
    feature_id: String,
) -> Result<
    (
        PlanningRequest,
        crate::harness::prompt_catalog::OutputValidationContext<'static>,
        PlanReviewOutcome,
    ),
    String,
> {
    use crate::harness::prompt_catalog::{
        EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, PromptRole, RoleInputV1,
        RolePayloadV1,
    };
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
    let rendered = crate::harness::prompt_catalog::render(&RoleInputV1 {
        role,
        workspace_uid: project
            .state
            .planning_store
            .project_id
            .hyphenated()
            .to_string(),
        feature_uid: identity.uid.clone(),
        task_uid: None,
        original_goal: snapshot.intent.clone(),
        approved_spec_ref: Some(snapshot.uid.clone()),
        approved_spec_hash: Some(snapshot.content_hash.clone()),
        decision_refs: Vec::new(),
        evidence_manifest: vec![EvidenceItem {
            id: format!("plan:{}", snapshot.uid),
            workspace_uid: project
                .state
                .planning_store
                .project_id
                .hyphenated()
                .to_string(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("immutable approved plan snapshot {}", snapshot.uid),
            content: serde_json::to_string(&snapshot).map_err(|error| error.to_string())?,
        }],
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
                .map(|req| format!("{}: {}", req.stable_id, req.text))
                .collect(),
            explicit_exclusions: snapshot.non_goals.clone(),
        },
    })
    .map_err(|error| error.to_string())?;
    let evidence_ids = [format!("plan:{}", snapshot.uid)].into_iter().collect();
    let context = crate::harness::prompt_catalog::OutputValidationContext {
        expected_subject_hash: None,
        evidence_ids,
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
