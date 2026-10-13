//! One bounded Planner v3 pass following a successfully reconciled clarification.
use crate::harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub(crate) struct ClarificationPlanner {
    result: mpsc::Receiver<Result<crate::harness::prompt_catalog::PlannerOutputV2, String>>,
    cancel: Arc<AtomicBool>,
}

impl ClarificationPlanner {
    pub(crate) fn start(
        project: &super::session::Project,
        feature_uid: String,
        plan_revision: u64,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let (progress_tx, _progress_rx) = mpsc::channel::<LiveProgress>();
        let (done_tx, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let prepared = render(project, &feature_uid, plan_revision);
        let repo_root = project.state.repo_root.clone();
        std::thread::spawn(move || {
            let outcome = (|| {
                let (rendered, validation) = prepared?;
                let request = PlanningRequest {
                    mode: ExecutionMode::ReadOnlyAnalysis,
                    task_id: None,
                    execution_workspace: None,
                    prompt_manifest: Some(rendered.manifest.clone()),
                    reasoning_level: "high".into(),
                    telemetry_phase: None,
                    repo_root,
                    runtime_config_source: None,
                    prompt_body: rendered.user,
                    system_instructions: rendered.system,
                    timeout: crate::core::turn::configured_turn_timeout(),
                    progress_tx,
                    cancel: worker_cancel,

                    authorization_verifier: None,
                    review_identity: None,
                };
                if request.cancel.load(Ordering::Relaxed) {
                    return Err("clarification planning was cancelled".into());
                }
                let response = harness.execute(&request).map_err(|error| error.detail())?;
                let crate::harness::prompt_catalog::RoleOutputV1::PlannerV3(output) =
                    crate::harness::prompt_catalog::validate_output_with_context(
                        crate::harness::prompt_catalog::PromptRole::Planner,
                        &response.final_text,
                        &validation,
                    )?
                else {
                    return Err("planner returned an unexpected schema".into());
                };
                output.validate()?;
                Ok(output)
            })();
            let _ = done_tx.send(outcome);
        });
        Self { result, cancel }
    }

    pub(crate) fn take_result(
        &self,
    ) -> Option<Result<crate::harness::prompt_catalog::PlannerOutputV2, String>> {
        self.result.try_recv().ok()
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for ClarificationPlanner {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(crate) fn spec_revision(
    project: &super::session::Project,
    feature_uid: &str,
) -> Result<String, String> {
    let (feature_id, markdown) = project
        .state
        .active_features
        .iter()
        .find(|(_, markdown)| {
            crate::domain::ArtifactIdentity::from_markdown(markdown)
                .ok()
                .flatten()
                .is_some_and(|identity| identity.uid == feature_uid)
        })
        .ok_or_else(|| "the clarified feature is no longer active".to_owned())?;
    let identity = crate::domain::ArtifactIdentity::from_markdown(markdown)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the clarified feature has no stable identity".to_owned())?;
    let contract = crate::core::workflow::feature_contract(markdown);
    crate::core::lifecycle::PlanSnapshotV1::from_feature(feature_id, &identity, markdown, &contract)
        .map(|snapshot| snapshot.spec_revision)
}

fn render(
    project: &super::session::Project,
    feature_uid: &str,
    plan_revision: u64,
) -> Result<
    (
        crate::harness::prompt_catalog::RenderedPrompt,
        crate::harness::prompt_catalog::OutputValidationContext<'static>,
    ),
    String,
> {
    use crate::harness::prompt_catalog::{
        EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, PromptRole, RoleInputV1,
        RolePayloadV1,
    };
    let feature_text = project
        .state
        .active_feature
        .as_ref()
        .filter(|(_, text)| {
            crate::domain::ArtifactIdentity::from_markdown(text)
                .ok()
                .flatten()
                .is_some_and(|identity| identity.uid == feature_uid)
        })
        .map(|(_, text)| text.as_str())
        .ok_or_else(|| "the clarified feature is no longer active".to_owned())?;
    let questions = crate::core::planning::QuestionStore::new(&project.state.planning_store)
        .list()
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|question| question.feature_uid == feature_uid)
        .collect::<Vec<_>>();
    let open_questions = questions
        .iter()
        .filter(|question| question.status == crate::core::planning::QuestionStatus::Open)
        .map(|question| format!("{}: {}", question.uid, question.question))
        .collect::<Vec<_>>();
    let previous_answers = questions
        .iter()
        .filter(|question| {
            matches!(
                question.status,
                crate::core::planning::QuestionStatus::Answered
                    | crate::core::planning::QuestionStatus::Declined
            )
        })
        .map(|question| {
            format!(
                "{}: {}",
                question.question,
                question
                    .answer_text
                    .as_deref()
                    .or(question.selected_option.as_deref())
                    .unwrap_or("(declined; no answer was inferred)")
            )
        })
        .collect::<Vec<_>>();
    let requirements = questions
        .iter()
        .flat_map(|question| question.origin_requirement_ids.iter().cloned())
        .collect::<std::collections::HashSet<_>>();
    let workspace_uid = project
        .state
        .planning_store
        .project_id
        .hyphenated()
        .to_string();
    let feature_evidence_id = format!("feature:{feature_uid}");
    let mut evidence_manifest = vec![EvidenceItem {
        id: feature_evidence_id.clone(),
        workspace_uid: workspace_uid.clone(),
        origin: EvidenceOrigin::Application,
        confidentiality: EvidenceConfidentiality::WorkspacePrivate,
        source: format!("planning feature specification {feature_uid}"),
        content: crate::core::context_build::clip(feature_text, 16 * 1024),
    }];
    let mut evidence_ids = std::collections::HashSet::from([feature_evidence_id.clone()]);
    for question in &questions {
        let id = format!("clarification:{}", question.uid);
        evidence_ids.insert(id.clone());
        evidence_manifest.push(EvidenceItem {
            id,
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("persisted clarification {}", question.uid),
            content: format!(
                "Question: {}\nReason: {}\nStatus: {:?}\nAnswer: {}\nSelected option: {}",
                question.question,
                question.reason,
                question.status,
                question.answer_text.as_deref().unwrap_or("(none)"),
                question.selected_option.as_deref().unwrap_or("(none)")
            ),
        });
    }
    let mut research_evidence = evidence_ids.iter().cloned().collect::<Vec<_>>();
    research_evidence.sort();
    let input = RoleInputV1 {
        role: PromptRole::Planner,
        workspace_uid: workspace_uid.clone(),
        feature_uid: feature_uid.to_owned(),
        task_uid: None,
        original_goal: crate::core::workflow::feature_contract(feature_text),
        approved_spec_ref: None,
        approved_spec_hash: None,
        decision_refs: questions
            .iter()
            .map(|question| question.uid.clone())
            .collect(),
        evidence_manifest,
        execution_policy_ref: "planner-policy/application-persistence-only/v1".into(),
        output_schema_version: PromptRole::Planner.output_schema_version().into(),
        payload: RolePayloadV1::PlannerOpportunity {
            open_questions,
            previous_answers,
            research_evidence,
            requested_scope: format!(
                "Reassess feature plan revision {plan_revision} after the recorded clarification; propose only material new questions and traceable readiness evidence."
            ),
            opportunity_origin: None,
            existing_support_evidence: vec![],
            workaround: None,
            user_value: None,
            confidentiality_class: EvidenceConfidentiality::WorkspacePrivate,
            concrete_target_repo: None,
        },
    };
    let rendered = crate::harness::prompt_catalog::render(&input)?;
    Ok((
        rendered,
        crate::harness::prompt_catalog::OutputValidationContext {
            evidence_ids,
            required_requirements: requirements.into_iter().collect(),
            ..Default::default()
        },
    ))
}

mod persist;
pub(crate) use persist::persist;
