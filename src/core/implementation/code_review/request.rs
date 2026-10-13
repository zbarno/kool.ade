use super::*;

#[cfg(test)]
#[path = "request/tests.rs"]
mod tests;

pub(super) struct CodeReviewRequestInput<'a> {
    pub state: &'a Implementation,
    pub feature_uid: &'a str,
    pub subject_hash: &'a str,
    pub approved_plan_hash: &'a str,
    pub diff: &'a str,
    pub spec: &'a str,
    pub report: &'a str,
    pub requirements: &'a [String],
    pub author: &'a ReviewExecutionIdentity,
    pub reviewer: &'a ReviewExecutionIdentity,
}

pub(super) fn build_request(
    input: CodeReviewRequestInput<'_>,
) -> anyhow::Result<(
    PlanningRequest,
    crate::harness::prompt_catalog::PromptManifest,
    std::collections::HashSet<String>,
)> {
    let CodeReviewRequestInput {
        state,
        feature_uid,
        subject_hash,
        approved_plan_hash,
        diff,
        spec,
        report,
        requirements,
        author,
        reviewer,
    } = input;
    use crate::harness::prompt_catalog::{
        EvidenceConfidentiality, EvidenceItem, EvidenceOrigin, PromptRole, RoleInputV1,
        RolePayloadV1,
    };
    let role = PromptRole::CodeReviewer;
    let workspace_uid = state
        .workspace_uid
        .clone()
        .or_else(|| state.project_id.clone())
        .unwrap_or_else(|| feature_uid.to_owned());
    let mut evidence = Vec::new();
    let mut evidence_ids = std::collections::HashSet::new();
    let mut add = |id: &str, source: &str, content: &str| -> anyhow::Result<()> {
        anyhow::ensure!(
            content.len() <= MAX_EVIDENCE_ITEM,
            "CodeReviewer evidence `{id}` exceeds the safe prompt bound"
        );
        evidence_ids.insert(id.to_owned());
        evidence.push(EvidenceItem {
            id: id.into(),
            workspace_uid: workspace_uid.clone(),
            origin: EvidenceOrigin::Application,
            confidentiality: EvidenceConfidentiality::WorkspacePrivate,
            source: source.into(),
            content: content.into(),
        });
        Ok(())
    };
    add("approved-task-spec", "approved task specification", spec)?;
    let mut offset = 0;
    let mut index = 0;
    while offset < diff.len() {
        let mut end = (offset + 8 * 1024).min(diff.len());
        while !diff.is_char_boundary(end) {
            end -= 1;
        }
        add(
            &format!("diff-{index}"),
            "complete immutable git diff; chunks form the full diff",
            &diff[offset..end],
        )?;
        index += 1;
        offset = end;
    }
    let mut report_offset = 0;
    let mut report_index = 0;
    while report_offset < report.len() {
        let mut end = (report_offset + 8 * 1024).min(report.len());
        while !report.is_char_boundary(end) {
            end -= 1;
        }
        add(
            &format!("verified-test-report-{report_index}"),
            "application-validated verification report; chunks form the complete report",
            &report[report_offset..end],
        )?;
        report_index += 1;
        report_offset = end;
    }
    let rendered = crate::harness::prompt_catalog::render(&RoleInputV1 {
        role,
        workspace_uid,
        feature_uid: feature_uid.into(),
        task_uid: state.task_uid.clone(),
        original_goal: crate::core::context_build::clip(&state.ticket_text, 4000),
        approved_spec_ref: Some("approved-task-specification".into()),
        approved_spec_hash: Some(approved_plan_hash.into()),
        decision_refs: Vec::new(),
        evidence_manifest: evidence,
        execution_policy_ref: "code-review/read-only-immutable-diff/v1".into(),
        output_schema_version: role.output_schema_version().into(),
        payload: RolePayloadV1::Review {
            subject_revision: format!(
                "{}...{}",
                state.base_commit,
                state.verified_head.as_deref().unwrap_or_default()
            ),
            subject_hash: subject_hash.into(),
            author_run_id: author.execution_id.clone(),
            reviewer_run_id: reviewer.execution_id.clone(),
            author_session_id: Some(author.session_id.clone()),
            reviewer_session_id: Some(reviewer.session_id.clone()),
            requirements_matrix: requirements.to_vec(),
            explicit_exclusions: Vec::new(),
        },
    })
    .map_err(anyhow::Error::msg)?;
    let (progress_tx, _progress_rx) = mpsc::channel();
    let request = PlanningRequest {
        mode: ExecutionMode::ReadOnlyAnalysis,
        task_id: state.task_uid.clone(),
        execution_workspace: None,
        authorization_verifier: None,
        prompt_manifest: Some(rendered.manifest.clone()),
        reasoning_level: "high".into(),
        telemetry_phase: Some("independent_code_review".into()),
        repo_root: state.task_repository.clone(),
        runtime_config_source: None,
        prompt_body: rendered.user,
        system_instructions: rendered.system,
        timeout: crate::core::turn::configured_turn_timeout(),
        progress_tx,
        cancel: Arc::new(AtomicBool::new(false)),
        review_identity: Some(reviewer.clone()),
    };
    Ok((request, rendered.manifest, evidence_ids))
}
