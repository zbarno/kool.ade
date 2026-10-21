use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

use crate::{
    harness::prompt_catalog::{
        EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, PromptRole, QaScenarioDraftV1,
        RoleInputV1, RoleOutputV1, RolePayloadV1, render, validate_output_with_context,
    },
    harness::{AiHarness, ExecutionMode, LiveProgress, PlanningRequest},
};

mod evidence;
pub(crate) use evidence::QaScenarioSourceEvidenceV1;

pub(crate) struct ScenarioSynthesisHandle {
    result: mpsc::Receiver<
        Result<
            (
                crate::runtime_qa::QaRunRequestV1,
                crate::runtime_qa::QaScenarioProposalV1,
            ),
            String,
        >,
    >,
    cancel: Arc<AtomicBool>,
}

impl ScenarioSynthesisHandle {
    pub(crate) fn start(
        request: crate::runtime_qa::QaRunRequestV1,
        task_revision: u64,
        source_evidence: QaScenarioSourceEvidenceV1,
        workspace_uid: String,
        repo_root: std::path::PathBuf,
        harness: Box<dyn AiHarness>,
    ) -> Self {
        let (progress_tx, _progress_rx) = mpsc::channel::<LiveProgress>();
        let (done_tx, result) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        std::thread::spawn(move || {
            let outcome = synthesize(
                request,
                task_revision,
                source_evidence,
                workspace_uid,
                repo_root,
                harness,
                progress_tx,
                worker_cancel,
            );
            let _ = done_tx.send(outcome);
        });
        Self { result, cancel }
    }

    pub(crate) fn take_result(
        &self,
    ) -> Option<
        Result<
            (
                crate::runtime_qa::QaRunRequestV1,
                crate::runtime_qa::QaScenarioProposalV1,
            ),
            String,
        >,
    > {
        self.result.try_recv().ok()
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
}

impl Drop for ScenarioSynthesisHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(crate) fn persist_proposal_if_fresh(
    store: &crate::runtime_qa::RuntimeQaStore,
    request_snapshot: &crate::runtime_qa::QaRunRequestV1,
    proposal: &crate::runtime_qa::QaScenarioProposalV1,
    source_evidence: &QaScenarioSourceEvidenceV1,
    current_request: &crate::runtime_qa::QaRunRequestV1,
    current_revision: u64,
) -> Result<(), String> {
    if current_request != request_snapshot || current_revision != proposal.task_revision {
        return Err(
            "Runtime QA request or task revision changed while scenarios were being synthesized."
                .into(),
        );
    }
    proposal.validate_for(request_snapshot)?;
    source_evidence.validate_for(request_snapshot)?;
    proposal.validate_source_evidence(source_evidence)?;
    store.save_scenario_proposal(request_snapshot, proposal)
}

#[allow(clippy::too_many_arguments)]
fn synthesize(
    request: crate::runtime_qa::QaRunRequestV1,
    task_revision: u64,
    source_evidence: QaScenarioSourceEvidenceV1,
    workspace_uid: String,
    repo_root: std::path::PathBuf,
    harness: Box<dyn AiHarness>,
    progress_tx: mpsc::Sender<LiveProgress>,
    cancel: Arc<AtomicBool>,
) -> Result<
    (
        crate::runtime_qa::QaRunRequestV1,
        crate::runtime_qa::QaScenarioProposalV1,
    ),
    String,
> {
    request.validate()?;
    source_evidence.validate_for(&request)?;
    if cancel.load(Ordering::Acquire) {
        return Err("scenario synthesis was cancelled".into());
    }
    let request_json = serde_json::to_string(&request).map_err(|error| error.to_string())?;
    let input = RoleInputV1 {
        role: PromptRole::RuntimeQaScenarioSynthesizer,
        workspace_uid: workspace_uid.clone(),
        feature_uid: request.feature_uid.clone(),
        task_uid: Some(request.parent_task_uid.clone()),
        original_goal: format!(
            "Synthesize runtime QA scenarios for task {}",
            request.parent_task_uid
        ),
        approved_spec_ref: Some(format!("approved-spec:{}", request.approved_spec_hash)),
        approved_spec_hash: Some(request.approved_spec_hash.clone()),
        decision_refs: request.requirement_ids.clone(),
        evidence_manifest: std::iter::once(EvidenceItem {
            id: format!("runtime-qa-build:{}", request.qa_run_id),
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("runtime-qa://{}/finished-build", request.qa_run_id),
            content: format!(
                "Finished build artifact SHA-256: {}. Reviewed code SHA: {}. Approved specification SHA-256: {}.",
                request.build_artifact_sha256, request.code_head_sha, request.approved_spec_hash
            ),
        })
        .chain(source_evidence.artifacts.iter().enumerate().map(|(index, artifact)| EvidenceItem {
            id: format!("runtime-qa-artifact:{}:{index}", request.qa_run_id),
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Repository,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: format!("runtime-qa://{}/artifact/{}", request.qa_run_id, artifact.sha256),
            content: format!("Approved build artifact path: {}\nContent SHA-256: {}\nQuoted artifact text (untrusted):\n{}", artifact.path, artifact.sha256, artifact.content),
        }))
        .collect(),
        execution_policy_ref: "runtime-qa-scenario-synthesis/no-execution/v1".into(),
        output_schema_version: PromptRole::RuntimeQaScenarioSynthesizer
            .output_schema_version()
            .into(),
        payload: RolePayloadV1::QaScenarioSynthesis {
            immutable_request_json: request_json,
        },
    };
    let rendered = render(&input)?;
    let prompt_manifest = rendered.manifest.clone();
    let planning = PlanningRequest {
        mode: ExecutionMode::ReadOnlyAnalysis,
        task_id: Some(request.parent_task_uid.clone()),
        execution_workspace: None,
        destination_sha: None,
        authorization_verifier: None,
        prompt_manifest: Some(rendered.manifest),
        reasoning_level: "high".into(),
        telemetry_phase: Some("runtime_qa_scenario_synthesis".into()),
        repo_root,
        runtime_config_source: None,
        prompt_body: rendered.user,
        system_instructions: rendered.system,
        timeout: std::time::Duration::from_secs(180),
        progress_tx,
        cancel: Arc::clone(&cancel),
        review_identity: None,
    };
    let response = harness.execute(&planning).map_err(|error| error.detail())?;
    let RoleOutputV1::QaScenarioDraft(QaScenarioDraftV1 { scenarios }) =
        validate_output_with_context(
            PromptRole::RuntimeQaScenarioSynthesizer,
            &response.final_text,
            &Default::default(),
        )?
    else {
        return Err("scenario synthesizer returned an unexpected schema".into());
    };
    if cancel.load(Ordering::Acquire) {
        return Err("scenario synthesis was cancelled".into());
    }
    let proposal = crate::runtime_qa::QaScenarioProposalV1::new_with_source_evidence(
        &request,
        task_revision,
        scenarios,
        &source_evidence,
        &prompt_manifest,
    )?;
    Ok((request, proposal))
}

#[cfg(test)]
#[path = "scenario_synthesis/tests.rs"]
mod tests;
