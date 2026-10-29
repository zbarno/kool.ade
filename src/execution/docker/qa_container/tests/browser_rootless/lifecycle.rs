use crate::{
    harness::prompt_catalog::{PromptRole, RoleInputV1, RolePayloadV1, render},
    runtime_qa::{
        QaDriverOutputV1, QaOutcome, QaPlatform, QaRunRequestV1, QaScenarioDriver, QaScenarioV1,
        QaTargetAuthorization, QaTaskServiceLeaseV1,
    },
};

#[path = "lifecycle/persist.rs"]
mod persist;
pub(super) use persist::review_expected_outcome_and_persist;

/// Production boundary: actual rootless Docker web or service evidence enters the
/// reviewed-report orchestration path and is durably stored with its receipt.
/// Unit tests cannot prove that screenshots and interactions from the pinned
/// browser image survive this boundary. External dependency: rootless Docker
/// and the candidate browser image. Setup: one short fixture run.
/// The persistence helper uses a deterministic reviewer test double; this path
/// does not prove separate AI reviewer execution or semantic reviewer quality.
/// Its output adapter replays the result from the preceding Docker driver run,
/// not a second container-driver implementation.
pub(super) fn review_and_persist(
    request: &QaRunRequestV1,
    scenario: QaScenarioV1,
    lease: &QaTaskServiceLeaseV1,
    output: QaDriverOutputV1,
    store_root: impl Into<std::path::PathBuf>,
) {
    review_expected_outcome_and_persist(
        request,
        scenario,
        lease,
        output,
        store_root,
        QaOutcome::Pass,
    );
}

fn reviewer_prompt(
    request: &QaRunRequestV1,
    scenario: &QaScenarioV1,
    reviewer_execution_id: &str,
    reviewer_session_id: &str,
) -> crate::harness::prompt_catalog::PromptManifest {
    let scenario_hash = crate::runtime_qa::scenario_set_sha256(std::slice::from_ref(scenario))
        .expect("the approved QA scenario hash is valid");
    render(&RoleInputV1 {
        role: PromptRole::RuntimeQaReviewerV3,
        workspace_uid: request.feature_uid.clone(),
        feature_uid: request.feature_uid.clone(),
        task_uid: Some(request.parent_task_uid.clone()),
        original_goal: request.acceptance_criteria.join("\n"),
        approved_spec_ref: None,
        approved_spec_hash: Some(request.approved_spec_hash.clone()),
        decision_refs: vec![],
        evidence_manifest: vec![],
        execution_policy_ref: "runtime-qa-review/rootless-fixture-v1".into(),
        output_schema_version: PromptRole::RuntimeQaReviewerV3
            .output_schema_version()
            .into(),
        payload: RolePayloadV1::Review {
            subject_revision: request.qa_run_id.clone(),
            subject_hash: scenario_hash,
            author_run_id: request.author_run_id.clone(),
            reviewer_run_id: reviewer_execution_id.to_owned(),
            author_session_id: Some(request.author_session_id.clone()),
            reviewer_session_id: Some(reviewer_session_id.to_owned()),
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
    .manifest
}

struct RootlessServiceOutput(Option<QaDriverOutputV1>, QaPlatform);

impl QaScenarioDriver for RootlessServiceOutput {
    fn platform(&self) -> QaPlatform {
        self.1
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reviewer_prompt_accepts_a_sha1_git_head_via_the_sha256_review_subject() {
        let mut request = super::super::super::request();
        request.code_head_sha = "a".repeat(40);
        request.review_subject_sha256 = "b".repeat(64);
        request.validate().unwrap();
        let scenario = super::super::scenario::browser_scenario(&request);
        let reviewer_execution_id = uuid::Uuid::new_v4().hyphenated().to_string();
        let reviewer_session_id = uuid::Uuid::new_v4().hyphenated().to_string();

        let prompt = reviewer_prompt(
            &request,
            &scenario,
            &reviewer_execution_id,
            &reviewer_session_id,
        );

        assert_eq!(
            crate::harness::prompt_catalog::validate_prompt_manifest(&prompt).unwrap(),
            PromptRole::RuntimeQaReviewerV3
        );
    }
}
