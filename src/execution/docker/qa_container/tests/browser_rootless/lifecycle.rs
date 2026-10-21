use crate::{
    harness::prompt_catalog::{
        CoverageStatus, CoverageV1, PromptRole, ReviewOutcomeV1, ReviewVerdictV1, RoleInputV1,
        RolePayloadV1, render,
    },
    runtime_qa::{
        QaDriverOutputV1, QaEvidenceManifestV1, QaOutcome, QaPlatform, QaResultV1, QaRunRequestV1,
        QaScenarioDriver, QaScenarioV1, QaTargetAuthorization, QaTaskServiceLeaseV1,
        RuntimeQaOrchestrator, RuntimeQaStore,
    },
};

/// Production boundary: actual rootless Docker browser evidence enters the
/// reviewed-report orchestration path and is durably stored with its receipt.
/// Unit tests cannot prove that screenshots and interactions from the pinned
/// browser image survive this boundary. External dependency: rootless Docker
/// and the candidate browser image. Setup: one short fixture run.
/// The reviewer closure below is a deterministic test double; this test does
/// not prove separate AI reviewer execution or semantic reviewer quality. The
/// browser output adapter replays the result captured by the preceding Docker
/// driver run; it is not a second container-driver implementation.
pub(super) fn review_and_persist(
    request: &QaRunRequestV1,
    scenario: QaScenarioV1,
    lease: &QaTaskServiceLeaseV1,
    output: QaDriverOutputV1,
    store_root: impl Into<std::path::PathBuf>,
) {
    let reviewer_execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let reviewer_session_id = uuid::Uuid::new_v4().hyphenated().to_string();
    let prompt = render(&RoleInputV1 {
        role: PromptRole::RuntimeQaReviewerV2,
        workspace_uid: request.feature_uid.clone(),
        feature_uid: request.feature_uid.clone(),
        task_uid: Some(request.parent_task_uid.clone()),
        original_goal: request.acceptance_criteria.join("\n"),
        approved_spec_ref: None,
        approved_spec_hash: Some(request.approved_spec_hash.clone()),
        decision_refs: vec![],
        evidence_manifest: vec![],
        execution_policy_ref: "runtime-qa-review/rootless-fixture-v1".into(),
        output_schema_version: PromptRole::RuntimeQaReviewerV2
            .output_schema_version()
            .into(),
        payload: RolePayloadV1::Review {
            subject_revision: request.qa_run_id.clone(),
            subject_hash: request.code_head_sha.clone(),
            author_run_id: request.author_run_id.clone(),
            reviewer_run_id: reviewer_execution_id.clone(),
            author_session_id: Some(request.author_session_id.clone()),
            reviewer_session_id: Some(reviewer_session_id.clone()),
            requirements_matrix: request
                .requirement_ids
                .iter()
                .zip(&request.acceptance_criteria)
                .map(|(requirement, criterion)| format!("{requirement}: {criterion}"))
                .collect(),
            explicit_exclusions: vec![],
        },
    })
    .expect("the pinned QA reviewer prompt is valid")
    .manifest;

    let mut driver = RootlessBrowserOutput(Some(output));
    let mut reviewer = |evidence: &QaEvidenceManifestV1, candidate: &QaResultV1| {
        let observed_pass = candidate.outcome == QaOutcome::Pass
            && !evidence.actions.is_empty()
            && !evidence.screenshots.is_empty()
            && evidence.browser_console.is_empty()
            && evidence.failed_requests.is_empty();
        Ok(ReviewVerdictV1 {
            subject_hash: evidence.sha256()?,
            examined_files: vec![],
            tested_evidence_ids: evidence
                .actions
                .iter()
                .map(|action| action.index.to_string())
                .collect(),
            unexamined_areas: vec![],
            pass: observed_pass,
            outcome: Some(if observed_pass {
                ReviewOutcomeV1::Pass
            } else {
                ReviewOutcomeV1::BlockingFindings
            }),
            coverage: request
                .requirement_ids
                .iter()
                .map(|requirement| CoverageV1 {
                    requirement: requirement.clone(),
                    status: if observed_pass {
                        CoverageStatus::Pass
                    } else {
                        CoverageStatus::Fail
                    },
                    evidence: evidence
                        .actions
                        .iter()
                        .map(|action| action.index.to_string())
                        .collect(),
                })
                .collect(),
            findings: vec![],
            untested: vec![],
        })
    };

    let store = RuntimeQaStore::new(store_root);
    let (result, receipt) = RuntimeQaOrchestrator::new(&store)
        .execute_reviewed(
            crate::runtime_qa::QaExecutionContext {
                request,
                scenarios: std::slice::from_ref(&scenario),
                authorization: QaTargetAuthorization::Web(lease),
                driver: &mut driver,
                reviewer_execution_id: &reviewer_execution_id,
                reviewer_session_id: &reviewer_session_id,
                prompt,
            },
            &mut reviewer,
        )
        .expect("the live browser evidence is reviewable and persistable");
    assert_eq!(result.outcome, QaOutcome::Pass);
    assert!(
        receipt.is_some(),
        "reviewed pass must produce a gate receipt"
    );

    let persisted = store
        .load_run_with_screenshots(&request.qa_run_id)
        .expect("the complete browser report and screenshot bytes are durable");
    assert_eq!(persisted.request, *request);
    assert_eq!(persisted.scenarios, vec![scenario]);
    assert_eq!(persisted.result, result);
    assert!(!persisted.screenshots.is_empty());
    assert_eq!(
        store.load_gate_receipt(&request.qa_run_id).unwrap(),
        receipt.expect("reviewed pass produced a gate receipt")
    );
}

struct RootlessBrowserOutput(Option<QaDriverOutputV1>);

impl QaScenarioDriver for RootlessBrowserOutput {
    fn platform(&self) -> QaPlatform {
        QaPlatform::Web
    }

    fn run_scenario(
        &mut self,
        request: &QaRunRequestV1,
        _scenario: &QaScenarioV1,
        _authorization: &QaTargetAuthorization<'_>,
    ) -> Result<QaDriverOutputV1, String> {
        let output = self
            .0
            .take()
            .ok_or_else(|| "rootless browser output was consumed more than once".to_owned())?;
        if output.driver_version != request.driver_profile_version {
            return Err("rootless browser evidence has an unexpected driver version".into());
        }
        Ok(output)
    }
}
